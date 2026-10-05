//! Terminals: a shell on this machine, and the seam a remote one will fit.
//!
//! A local terminal is a shell behind a Windows pseudoconsole (ConPTY), driven
//! through the OS API directly. That is a few hundred lines against a
//! dependency that would pull in a dozen crates to do the same calls.
//!
//! The window knows a terminal only as a [`Terminal`]: something to write to
//! and resize, that ends when it is dropped. A shell on a remote host is a
//! second implementation of that, not a change to anything that uses it.

use std::ffi::{c_void, OsStr};
use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, InitializeProcThreadAttributeList, ResumeThread,
    TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject, CREATE_SUSPENDED,
    CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, INFINITE,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW,
};

use crate::{Error, Result};

/// A running terminal, wherever its shell is. Dropping it ends the shell.
pub trait Terminal: Send {
    /// Send input. Returns once it is queued, not once the shell has read it.
    fn write(&self, data: &[u8]);
    fn resize(&self, cols: u16, rows: u16);
}

/// How to start a local shell.
#[derive(Debug, Clone)]
pub struct Options {
    /// The command line to run. Empty means [`default_shell`].
    pub shell: String,
    /// Where the shell starts. A folder that is not there falls back to home.
    pub cwd: Option<PathBuf>,
    pub cols: u16,
    pub rows: u16,
}

/// The command line for the shell to use when the config names none:
/// PowerShell 7 where it is installed, Windows PowerShell otherwise.
pub fn default_shell() -> String {
    let dir = |var: &str, fallback: &str| {
        std::env::var_os(var)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(fallback))
    };
    let system = dir("SystemRoot", r"C:\Windows").join("System32");

    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|dir| dir.join("pwsh.exe"));

    std::iter::once(dir("ProgramFiles", r"C:\Program Files").join(r"PowerShell\7\pwsh.exe"))
        .chain(on_path)
        .chain([system.join(r"WindowsPowerShell\v1.0\powershell.exe")])
        .find(|candidate| candidate.is_file())
        .map(|shell| format!("\"{}\" -NoLogo", shell.display()))
        .unwrap_or_else(|| system.join("cmd.exe").display().to_string())
}

/// A shell on this machine, behind a pseudoconsole.
pub struct LocalTerminal {
    console: Arc<Console>,
    /// Input is queued for a thread that owns the pipe; see [`LocalTerminal::spawn`].
    input: mpsc::Sender<Vec<u8>>,
    process: Arc<OwnedHandle>,
    /// Closing this kills whatever the shell started and left running.
    _job: Option<OwnedHandle>,
}

impl LocalTerminal {
    /// Start a shell, handing its output to `on_data` as it arrives.
    ///
    /// `on_exit` runs once, after the last of the output, when the shell has
    /// ended -- by itself or by this being dropped.
    pub fn spawn<D, E>(options: Options, mut on_data: D, on_exit: E) -> Result<Self>
    where
        D: FnMut(&[u8]) + Send + 'static,
        E: FnOnce() + Send + 'static,
    {
        let (input_read, input_write) = pipe()?;
        let (output_read, output_write) = pipe()?;

        // SAFETY: both handles are live pipe ends for the duration of the call.
        let console = unsafe {
            CreatePseudoConsole(
                size(options.cols, options.rows),
                raw(&input_read),
                raw(&output_write),
                0,
            )
        }
        .map(|hpc| Arc::new(Console(Mutex::new(Some(hpc)))))
        .map_err(os)?;
        // The pseudoconsole has its own copies of these now. Ours must go, or
        // the output pipe never reports the end when the console closes.
        drop((input_read, output_write));

        let (process, job) = launch(&options, &console)?;
        let process = Arc::new(process);

        // One read is whatever has piled up since the last, so a flood reaches
        // `on_data` in large pieces with nothing here having to batch it.
        let mut output = File::from(output_read);
        thread::spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            while let Ok(n @ 1..) = output.read(&mut buf) {
                on_data(&buf[..n]);
            }
            on_exit();
        });

        // The end is the *process* ending, not the output pipe: the
        // pseudoconsole holds that open for as long as it lives, so a shell
        // that ran `exit` would otherwise look exactly like one that hung.
        // Closing the console here is what ends the read above.
        thread::spawn({
            let (console, process) = (Arc::clone(&console), Arc::clone(&process));
            move || {
                // SAFETY: the handle is kept alive by the `Arc` held here.
                unsafe { WaitForSingleObject(raw(&process), INFINITE) };
                console.close();
            }
        });

        // Writes are queued rather than made by the caller: a program that has
        // stopped reading its input fills the pipe, and the write that then
        // blocks would otherwise be on the window's own thread.
        let (input, queued) = mpsc::channel::<Vec<u8>>();
        let mut pipe = File::from(input_write);
        thread::spawn(move || {
            while let Ok(mut data) = queued.recv() {
                // A paste arrives in pieces; whatever is waiting goes together.
                while let Ok(more) = queued.try_recv() {
                    data.extend_from_slice(&more);
                }
                if pipe.write_all(&data).is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            console,
            input,
            process,
            _job: job,
        })
    }
}

impl Terminal for LocalTerminal {
    fn write(&self, data: &[u8]) {
        // Refused only once the shell has gone, which is not worth reporting.
        let _ = self.input.send(data.to_vec());
    }

    fn resize(&self, cols: u16, rows: u16) {
        self.console.resize(cols, rows);
    }
}

impl Drop for LocalTerminal {
    fn drop(&mut self) {
        // The shell here; what it started goes when the job closes after this.
        // SAFETY: the handle is owned by `self`.
        let _ = unsafe { TerminateProcess(raw(&self.process), 1) };
    }
}

/// The pseudoconsole, closed once and never used after.
struct Console(Mutex<Option<HPCON>>);

impl Console {
    fn resize(&self, cols: u16, rows: u16) {
        if let Ok(guard) = self.0.lock() {
            if let Some(hpc) = *guard {
                // SAFETY: `hpc` is open for as long as it is in the mutex.
                let _ = unsafe { ResizePseudoConsole(hpc, size(cols, rows)) };
            }
        }
    }

    fn close(&self) {
        let hpc = self.0.lock().ok().and_then(|mut guard| guard.take());
        if let Some(hpc) = hpc {
            // SAFETY: taken out of the mutex, so this is the only close.
            unsafe { ClosePseudoConsole(hpc) };
        }
    }
}

impl Drop for Console {
    fn drop(&mut self) {
        self.close();
    }
}

/// Start the shell attached to `console`, inside a job of its own.
fn launch(options: &Options, console: &Console) -> Result<(OwnedHandle, Option<OwnedHandle>)> {
    let hpc = console
        .0
        .lock()
        .ok()
        .and_then(|guard| *guard)
        .ok_or_else(|| Error::Other(anyhow::anyhow!("the console closed before the shell ran")))?;

    // The attribute list is an opaque block whose size is asked for first; the
    // sizing call fails by design. `usize`s so the block is pointer-aligned.
    let mut bytes = 0usize;
    // SAFETY: a null list with a size out-pointer is the documented sizing form.
    let _ = unsafe { InitializeProcThreadAttributeList(None, 1, None, &mut bytes) };
    let mut block = vec![0usize; bytes.div_ceil(size_of::<usize>())];
    let attributes = LPPROC_THREAD_ATTRIBUTE_LIST(block.as_mut_ptr().cast());

    // SAFETY: `block` is at least `bytes` long and outlives the list.
    unsafe { InitializeProcThreadAttributeList(Some(attributes), 1, None, &mut bytes) }
        .map_err(os)?;
    // SAFETY: for this attribute the value is the console handle itself.
    let attached = unsafe {
        UpdateProcThreadAttribute(
            attributes,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            Some(hpc.0 as *const c_void),
            size_of::<HPCON>(),
            None,
            None,
        )
    };

    let startup = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: size_of::<STARTUPINFOEXW>() as u32,
            // Explicitly no standard handles. Left unsaid, a Thread started
            // from a console or with its output redirected hands *those* to
            // the shell, which then talks to them instead of the pseudoconsole.
            dwFlags: STARTF_USESTDHANDLES,
            hStdInput: INVALID_HANDLE_VALUE,
            hStdOutput: INVALID_HANDLE_VALUE,
            hStdError: INVALID_HANDLE_VALUE,
            ..Default::default()
        },
        lpAttributeList: attributes,
    };

    let shell = match options.shell.trim() {
        "" => default_shell(),
        shell => shell.to_owned(),
    };
    // CreateProcess may write to the command line it is given.
    let mut command = wide(OsStr::new(&shell));
    let cwd = wide(start_dir(options.cwd.as_deref()).as_os_str());
    let environment = environment();
    let mut info = PROCESS_INFORMATION::default();

    let created = attached.and_then(|()| {
        // SAFETY: every pointer is to a NUL-terminated buffer or a struct
        // that lives until the call returns.
        unsafe {
            CreateProcessW(
                PCWSTR::null(),
                Some(PWSTR(command.as_mut_ptr())),
                None,
                None,
                false,
                // Suspended, so it is in its job before it can start anything
                // that would otherwise be left outside it.
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_SUSPENDED,
                Some(environment.as_ptr().cast()),
                PCWSTR(cwd.as_ptr()),
                &startup.StartupInfo,
                &mut info,
            )
        }
    });
    // SAFETY: initialised above, and not used again.
    unsafe { DeleteProcThreadAttributeList(attributes) };
    created.map_err(|e| Error::Other(anyhow::anyhow!("could not start `{shell}`: {e}")))?;

    // SAFETY: CreateProcess succeeded, so both are handles this now owns.
    let (process, main_thread) = unsafe {
        (
            OwnedHandle::from_raw_handle(info.hProcess.0),
            OwnedHandle::from_raw_handle(info.hThread.0),
        )
    };

    let job = job_for(&process)
        .inspect_err(|e| {
            tracing::warn!("the shell is not in a job; what it starts may outlive it: {e}")
        })
        .ok();
    // SAFETY: the thread handle is owned here and still open.
    unsafe { ResumeThread(raw(&main_thread)) };

    Ok((process, job))
}

/// A job holding `process` that kills everything in it when closed.
///
/// Ending a shell does not end what the shell started: a build or a server it
/// launched would carry on with nothing left to stop it from. In a job, the
/// whole tree goes with the terminal.
fn job_for(process: &OwnedHandle) -> Result<OwnedHandle> {
    // SAFETY: no attributes and no name is the documented default form.
    let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(os)?;
    // SAFETY: just created, and owned from here.
    let job = unsafe { OwnedHandle::from_raw_handle(job.0) };

    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: `limits` is the struct this information class takes.
    unsafe {
        SetInformationJobObject(
            raw(&job),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    }
    .map_err(os)?;
    // SAFETY: both handles are open.
    unsafe { AssignProcessToJobObject(raw(&job), raw(process)) }.map_err(os)?;
    Ok(job)
}

/// The folder asked for if it is there, and home if not: a shell that will not
/// start because a folder was deleted is no use to anyone.
fn start_dir(cwd: Option<&Path>) -> PathBuf {
    cwd.filter(|dir| dir.is_dir())
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(r"C:\"))
}

/// This process's environment, as the block CreateProcess takes, plus the one
/// thing a program cannot work out by asking: that 24-bit colour is drawn.
/// Without it a colourscheme falls back to its 256-colour approximation.
fn environment() -> Vec<u16> {
    let mut block = Vec::new();
    for (key, value) in std::env::vars_os() {
        if key.eq_ignore_ascii_case("COLORTERM") {
            continue;
        }
        block.extend(key.encode_wide());
        block.push(u16::from(b'='));
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.extend("COLORTERM=truecolor\0".encode_utf16());
    block.push(0);
    block
}

fn pipe() -> Result<(OwnedHandle, OwnedHandle)> {
    let (mut read, mut write) = (HANDLE::default(), HANDLE::default());
    // SAFETY: both out-pointers are to locals.
    unsafe { CreatePipe(&mut read, &mut write, None, 0) }.map_err(os)?;
    // SAFETY: CreatePipe succeeded, so both are handles this now owns.
    Ok(unsafe {
        (
            OwnedHandle::from_raw_handle(read.0),
            OwnedHandle::from_raw_handle(write.0),
        )
    })
}

fn raw(handle: &OwnedHandle) -> HANDLE {
    HANDLE(handle.as_raw_handle())
}

fn size(cols: u16, rows: u16) -> COORD {
    // A console of no size is refused; one beyond an i16 cannot be said.
    let fit = |n: u16| n.clamp(1, i16::MAX as u16) as i16;
    COORD {
        X: fit(cols),
        Y: fit(rows),
    }
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain([0]).collect()
}

fn os(error: windows::core::Error) -> Error {
    Error::Io(error.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    /// Type `line` at a `cmd.exe` and wait for `done` to say it has finished.
    ///
    /// A pseudoconsole opens by asking where the cursor is (`ESC[6n`) and may
    /// wait for the answer, which is the terminal emulator's to give; here the
    /// test has to play that part.
    fn run(line: &str, done: impl Fn(&str, bool) -> bool) -> String {
        let seen = Arc::new(Mutex::new(Vec::<u8>::new()));
        let exited = Arc::new(AtomicBool::new(false));

        let terminal = LocalTerminal::spawn(
            Options {
                shell: "cmd.exe".into(),
                cwd: None,
                cols: 80,
                rows: 24,
            },
            {
                let seen = Arc::clone(&seen);
                move |chunk| seen.lock().unwrap().extend_from_slice(chunk)
            },
            {
                let exited = Arc::clone(&exited);
                move || exited.store(true, Ordering::SeqCst)
            },
        )
        .expect("spawn cmd.exe");

        let start = Instant::now();
        let (mut answered, mut typed) = (false, false);
        loop {
            let text = String::from_utf8_lossy(&seen.lock().unwrap()).into_owned();
            if !answered && text.contains("\x1b[6n") {
                terminal.write(b"\x1b[1;1R");
                answered = true;
            }
            // Not held up forever by a console that never asks.
            if !typed && (answered || start.elapsed() > Duration::from_secs(3)) {
                terminal.write(format!("{line}\r\n").as_bytes());
                typed = true;
            }
            if done(&text, exited.load(Ordering::SeqCst)) {
                return text;
            }
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "timed out; the output was:\n{text:?}"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn a_command_goes_in_and_its_output_comes_back() {
        // Twice: once as the console echoes what was typed, once as the output.
        run("echo thread-terminal-ok", |text, _| {
            text.matches("thread-terminal-ok").count() >= 2
        });
    }

    /// `exit` has to end the terminal by itself, or the panel would sit there
    /// on a shell that has gone.
    #[test]
    fn exit_ends_the_terminal() {
        run("exit", |_, exited| exited);
    }

    #[test]
    fn dropping_a_terminal_ends_its_shell() {
        let exited = Arc::new(AtomicBool::new(false));
        let terminal = LocalTerminal::spawn(
            Options {
                shell: "cmd.exe".into(),
                cwd: None,
                cols: 80,
                rows: 24,
            },
            |_| {},
            {
                let exited = Arc::clone(&exited);
                move || exited.store(true, Ordering::SeqCst)
            },
        )
        .expect("spawn cmd.exe");
        drop(terminal);

        let start = Instant::now();
        while !exited.load(Ordering::SeqCst) {
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "the shell outlived its terminal"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn a_shell_that_does_not_exist_is_an_error_naming_it() {
        let error = LocalTerminal::spawn(
            Options {
                shell: "no-such-shell-thread.exe".into(),
                cwd: None,
                cols: 80,
                rows: 24,
            },
            |_| {},
            || {},
        )
        .err()
        .expect("nothing to start")
        .to_string();
        assert!(error.contains("no-such-shell-thread.exe"), "got {error}");
    }

    #[test]
    fn a_folder_that_has_gone_falls_back_to_one_that_exists() {
        assert!(start_dir(Some(Path::new(r"Z:\no\such\folder"))).is_dir());
    }

    #[test]
    fn the_default_shell_is_one_that_is_installed() {
        let shell = default_shell();
        let program = shell
            .strip_prefix('"')
            .and_then(|rest| rest.split('"').next())
            .unwrap_or(&shell);
        assert!(Path::new(program).is_file(), "got {shell}");
    }
}
