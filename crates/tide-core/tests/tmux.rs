//! The tmux engine against a real tmux.
//!
//! There is no tmux on Windows, so this borrows one from WSL, and only when
//! told which distribution has it:
//!
//! ```text
//! TIDE_TMUX_WSL=main cargo test -p tide-core --test tmux -- --ignored
//! ```
//!
//! It runs a server of its own, on its own socket and with no config, and
//! ends it afterwards: no session of anyone's is touched.

use std::io::{Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use tide_core::tmux::{Action, Dir, Engine, Event, Out, Start, State};

const SOCKET: &str = "tide-test";

fn tmux(distro: &str, arguments: &str) -> Command {
    let mut command = Command::new("wsl.exe");
    command.args(["-d", distro, "--", "sh", "-c"]);
    command.arg(format!("exec tmux -L {SOCKET} -f /dev/null {arguments}"));
    command
}

/// A tmux in control mode, an engine reading it, and what it has shown.
struct Attached {
    child: Child,
    input: ChildStdin,
    output: Receiver<Vec<u8>>,
    engine: Engine,
    state: Option<State>,
    /// Everything each pane has been shown, by pane.
    shown: std::collections::HashMap<u32, Vec<u8>>,
    errors: Vec<String>,
}

impl Attached {
    fn start(distro: &str) -> Self {
        let mut child = tmux(distro, &Start::New("first".into()).arguments(100, 30))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("wsl.exe");
        let input = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (send, output) = channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            while let Ok(n @ 1..) = stdout.read(&mut buf) {
                if send.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            output,
            engine: Engine::new(100, 30, 200),
            state: None,
            shown: Default::default(),
            errors: Vec::new(),
        }
    }

    fn take(&mut self, out: Out) {
        if !out.commands.is_empty() {
            self.input.write_all(&out.commands).unwrap();
            self.input.flush().unwrap();
        }
        for event in out.events {
            match event {
                Event::State(state) => self.state = Some(state),
                Event::Sizes(_) => {}
                Event::Output { pane, data } => {
                    self.shown.entry(pane).or_default().extend_from_slice(&data)
                }
                Event::Error(message) => self.errors.push(message),
            }
        }
    }

    fn act(&mut self, action: Action) {
        let mut out = Out::default();
        self.engine.act(action, &mut out);
        self.take(out);
    }

    /// Read what tmux says until `done`, or give up and say what there was.
    fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "never {what}; errors {:?}, state {:#?}",
                self.errors,
                self.state
            );
            if let Ok(bytes) = self.output.recv_timeout(Duration::from_millis(100)) {
                let mut out = Out::default();
                self.engine.feed(&bytes, &mut out);
                self.take(out);
            }
        }
    }

    fn panes(&self) -> Vec<u32> {
        let state = self.state.as_ref().expect("a state");
        let window = state.windows.iter().find(|window| window.active).unwrap();
        window.panes.iter().map(|pane| pane.id).collect()
    }

    fn shown(&self, pane: u32) -> String {
        String::from_utf8_lossy(self.shown.get(&pane).map_or(&[][..], Vec::as_slice)).into_owned()
    }
}

#[test]
#[ignore = "needs a tmux in WSL: set TIDE_TMUX_WSL to the distribution"]
fn a_session_is_made_split_typed_into_resized_and_ended() {
    let Ok(distro) = std::env::var("TIDE_TMUX_WSL") else {
        panic!("TIDE_TMUX_WSL is not set");
    };
    let _ = tmux(&distro, "kill-server").stderr(Stdio::null()).status();

    let mut tmux_ = Attached::start(&distro);
    tmux_.until("attached", |t| t.state.is_some());
    let state = tmux_.state.clone().unwrap();
    assert_eq!(state.sessions.len(), 1);
    assert_eq!(state.sessions[0].name, "first");
    assert_eq!(state.windows.len(), 1);
    let first = tmux_.panes()[0];
    assert_eq!((state.windows[0].cols, state.windows[0].rows), (100, 30));

    // A pane with a terminal is shown whole, and then hears what is typed.
    tmux_.act(Action::Watch { pane: first });
    tmux_.until("shown", |t| t.shown(first).starts_with("\x1bc"));
    tmux_.act(Action::Write {
        pane: first,
        text: "echo thr''ead-h\u{e9}\r".into(),
    });
    tmux_.until("echoed", |t| t.shown(first).contains("tide-h\u{e9}"));

    // Split, and the layout says so; the new pane prints to no one.
    tmux_.act(Action::Split {
        pane: first,
        dir: Dir::Row,
    });
    tmux_.until("split", |t| t.panes().len() == 2);
    let window = tmux_.state.as_ref().unwrap().windows[0].clone();
    assert_eq!(window.dividers.len(), 1);
    let second = tmux_.panes()[1];
    assert!(tmux_.shown(second).is_empty());

    // A dragged line is a layout tmux takes as its own.
    let before = window.panes[0].w;
    tmux_.act(Action::MoveDivider {
        window: window.id,
        divider: window.dividers[0].id.clone(),
        delta: -7,
    });
    tmux_.until("resized", |t| {
        t.state.as_ref().unwrap().windows[0].panes[0].w == before - 7
    });

    // The room changing is every pane's size changing.
    tmux_.act(Action::Resize {
        cols: 120,
        rows: 40,
    });
    tmux_.until("grown", |t| {
        let window = &t.state.as_ref().unwrap().windows[0];
        (window.cols, window.rows) == (120, 40)
    });

    // A second session, gone to as it is made; then the first one ended
    // from there, which leaves this attached.
    tmux_.act(Action::NewSession {
        name: "second".into(),
    });
    tmux_.until("switched", |t| {
        let state = t.state.as_ref().unwrap();
        state.sessions.len() == 2
            && state
                .sessions
                .iter()
                .find(|s| Some(s.id) == state.session)
                .map(|s| s.name.as_str())
                == Some("second")
    });
    let old = tmux_
        .state
        .as_ref()
        .unwrap()
        .sessions
        .iter()
        .find(|s| s.name == "first")
        .unwrap()
        .id;
    tmux_.act(Action::KillSession { session: old });
    tmux_.until("killed", |t| t.state.as_ref().unwrap().sessions.len() == 1);

    // A name already taken is refused, and the refusal is passed on.
    tmux_.act(Action::NewSession {
        name: "second".into(),
    });
    tmux_.until("refused", |t| !t.errors.is_empty());
    assert!(tmux_.errors[0].contains("duplicate"), "{:?}", tmux_.errors);
    assert_eq!(tmux_.errors.len(), 1, "{:?}", tmux_.errors);

    // The last session ending is tmux letting go.
    let last = tmux_.state.as_ref().unwrap().session.unwrap();
    tmux_.act(Action::KillSession { session: last });
    // How it says so is the server's business: with it goes the server.
    tmux_.child.wait().expect("tmux to end");
    let _ = tmux(&distro, "kill-server").stderr(Stdio::null()).status();
}
