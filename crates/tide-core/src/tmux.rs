//! tmux, spoken to in its control mode.
//!
//! `tmux -C` is tmux with no screen of its own: instead of drawing its panes
//! and its status line into one terminal, it says what each pane printed and
//! what changed, a line at a time, and takes its own commands the same way.
//! That is what lets a window draw a session as its own thing -- a pane as a
//! terminal, a tmux window as a tab -- while tmux stays what keeps it all
//! running when the window, or the connection, has gone.
//!
//! This is the half of that with no connection in it: an [`Engine`] is given
//! what tmux wrote and says what to show and what to write back. tmux is
//! asked what there is whenever it says something moved, rather than each of
//! its notices being applied to a copy kept here: there is then only the one
//! account of a session, and it is tmux's.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

/// How far behind a pane's output may fall before tmux skips to where the
/// pane is now, in seconds. The pane is then shown again from tmux's own
/// copy of it, so nothing is lost but the scrolling past.
const PAUSE_AFTER: u32 = 5;

/// The most bytes typed that go to tmux as one command.
const KEYS_PER_COMMAND: usize = 200;

/// Which way a split divides its space: `Row` is panes side by side, and
/// `Column` one above the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Dir {
    Row,
    Column,
}

// --- layouts ------------------------------------------------------------------

/// A box of a window's layout, in character cells: a pane, or a split of
/// boxes. Between two boxes of a split tmux leaves one cell, for the line it
/// would draw there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub w: u16,
    pub h: u16,
    pub x: u16,
    pub y: u16,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Pane(u32),
    Split(Dir, Vec<Cell>),
}

/// Which edge of a box a resize moves.
#[derive(Clone, Copy)]
enum Edge {
    Start,
    End,
}

impl Cell {
    /// Read a layout as tmux writes one: `77dd,100x30,0,0{50x30,0,0,0,...}`.
    pub fn parse(layout: &str) -> Option<Cell> {
        let (_checksum, body) = layout.split_once(',')?;
        let mut rest = body.as_bytes();
        let cell = Self::read(&mut rest)?;
        rest.is_empty().then_some(cell)
    }

    fn read(rest: &mut &[u8]) -> Option<Cell> {
        fn number(rest: &mut &[u8]) -> Option<u32> {
            let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
            let value = std::str::from_utf8(&rest[..digits]).ok()?.parse().ok()?;
            *rest = &rest[digits..];
            Some(value)
        }
        fn skip(rest: &mut &[u8], byte: u8) -> Option<()> {
            (rest.first() == Some(&byte)).then(|| *rest = &rest[1..])
        }

        let w = number(rest)? as u16;
        skip(rest, b'x')?;
        let h = number(rest)? as u16;
        skip(rest, b',')?;
        let x = number(rest)? as u16;
        skip(rest, b',')?;
        let y = number(rest)? as u16;

        let kind = match rest.first()? {
            b',' => {
                *rest = &rest[1..];
                Kind::Pane(number(rest)?)
            }
            open @ (b'{' | b'[') => {
                let (dir, close) = match open {
                    b'{' => (Dir::Row, b'}'),
                    _ => (Dir::Column, b']'),
                };
                *rest = &rest[1..];
                let mut cells = vec![Self::read(rest)?];
                while skip(rest, b',').is_some() {
                    cells.push(Self::read(rest)?);
                }
                skip(rest, close)?;
                Kind::Split(dir, cells)
            }
            _ => return None,
        };
        Some(Cell { w, h, x, y, kind })
    }

    /// The layout as tmux would write it, checksum and all: what
    /// `select-layout` takes.
    pub fn serialize(&self) -> String {
        let mut body = String::new();
        self.write(&mut body);
        let checksum = body.bytes().fold(0u16, |sum, byte| {
            ((sum >> 1) + ((sum & 1) << 15)).wrapping_add(u16::from(byte))
        });
        format!("{checksum:04x},{body}")
    }

    fn write(&self, out: &mut String) {
        use std::fmt::Write;
        let _ = write!(out, "{}x{},{},{}", self.w, self.h, self.x, self.y);
        match &self.kind {
            Kind::Pane(id) => {
                let _ = write!(out, ",{id}");
            }
            Kind::Split(dir, cells) => {
                let (open, close) = match dir {
                    Dir::Row => ('{', '}'),
                    Dir::Column => ('[', ']'),
                };
                out.push(open);
                for (i, cell) in cells.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    cell.write(out);
                }
                out.push(close);
            }
        }
    }

    /// Every pane in it, with the box it has.
    fn panes<'a>(&'a self, into: &mut Vec<(u32, &'a Cell)>) {
        match &self.kind {
            Kind::Pane(id) => into.push((*id, self)),
            Kind::Split(_, cells) => cells.iter().for_each(|cell| cell.panes(into)),
        }
    }

    fn size(&self, along: Dir) -> u16 {
        match along {
            Dir::Row => self.w,
            Dir::Column => self.h,
        }
    }

    /// How many cells one edge of this can give up along `along` before a
    /// pane in it is down to a single cell.
    fn slack(&self, along: Dir, edge: Edge) -> u16 {
        match &self.kind {
            Kind::Pane(_) => self.size(along).saturating_sub(1),
            // Only the box at that edge gives; the rest stay as they are.
            Kind::Split(dir, cells) if *dir == along => match edge {
                Edge::Start => cells.first(),
                Edge::End => cells.last(),
            }
            .map_or(0, |cell| cell.slack(along, edge)),
            Kind::Split(_, cells) => cells
                .iter()
                .map(|cell| cell.slack(along, edge))
                .min()
                .unwrap_or(0),
        }
    }

    /// Move one edge by `by` cells along `along`: positive is towards the
    /// right or the bottom. Everything inside follows.
    fn shift(&mut self, along: Dir, edge: Edge, by: i32) {
        let moved = |value: u16, by: i32| (i32::from(value) + by).max(0) as u16;
        let (size, at) = match along {
            Dir::Row => (&mut self.w, &mut self.x),
            Dir::Column => (&mut self.h, &mut self.y),
        };
        match edge {
            Edge::End => *size = moved(*size, by),
            Edge::Start => {
                *at = moved(*at, by);
                *size = moved(*size, -by);
            }
        }
        if let Kind::Split(dir, cells) = &mut self.kind {
            if *dir == along {
                let at_edge = match edge {
                    Edge::Start => cells.first_mut(),
                    Edge::End => cells.last_mut(),
                };
                if let Some(cell) = at_edge {
                    cell.shift(along, edge, by);
                }
            } else {
                cells
                    .iter_mut()
                    .for_each(|cell| cell.shift(along, edge, by));
            }
        }
    }

    /// The split at the end of `path`, each step of which is a place in the
    /// split before it.
    fn split_at(&mut self, path: &[usize]) -> Option<(Dir, &mut Vec<Cell>)> {
        match (&mut self.kind, path) {
            (Kind::Split(dir, cells), []) => Some((*dir, cells)),
            (Kind::Split(_, cells), [first, rest @ ..]) => cells.get_mut(*first)?.split_at(rest),
            (Kind::Pane(_), _) => None,
        }
    }

    /// Drag the line a [`Divider`] names by `delta` cells, as far as the
    /// panes either side of it allow. False if there is no such line, or it
    /// did not move.
    pub fn move_divider(&mut self, divider: &str, delta: i32) -> bool {
        let Some((path, index)) = divider.split_once(':') else {
            return false;
        };
        let Ok(path) = path
            .split('.')
            .filter(|step| !step.is_empty())
            .map(str::parse)
            .collect::<Result<Vec<usize>, _>>()
        else {
            return false;
        };
        let (Ok(index), Some((dir, cells))) = (index.parse::<usize>(), self.split_at(&path)) else {
            return false;
        };
        if index + 1 >= cells.len() {
            return false;
        }
        let lo = -i32::from(cells[index].slack(dir, Edge::End));
        let hi = i32::from(cells[index + 1].slack(dir, Edge::Start));
        let delta = delta.clamp(lo, hi);
        if delta == 0 {
            return false;
        }
        cells[index].shift(dir, Edge::End, delta);
        cells[index + 1].shift(dir, Edge::Start, delta);
        true
    }

    /// The lines between its boxes that can be dragged.
    fn dividers(&self, path: &mut Vec<usize>, into: &mut Vec<Divider>) {
        let Kind::Split(dir, cells) = &self.kind else {
            return;
        };
        let steps: Vec<String> = path.iter().map(usize::to_string).collect();
        for (index, pair) in cells.windows(2).enumerate() {
            let (before, after) = (&pair[0], &pair[1]);
            into.push(Divider {
                id: format!("{}:{index}", steps.join(".")),
                dir: *dir,
                // The one cell tmux leaves between them.
                x: match dir {
                    Dir::Row => before.x + before.w,
                    Dir::Column => self.x,
                },
                y: match dir {
                    Dir::Row => self.y,
                    Dir::Column => before.y + before.h,
                },
                length: match dir {
                    Dir::Row => self.h,
                    Dir::Column => self.w,
                },
                lo: -i32::from(before.slack(*dir, Edge::End)),
                hi: i32::from(after.slack(*dir, Edge::Start)),
            });
        }
        for (index, cell) in cells.iter().enumerate() {
            path.push(index);
            cell.dividers(path, into);
            path.pop();
        }
    }
}

// --- what there is --------------------------------------------------------------

/// Everything a window needs to draw the session it is attached to: the
/// sessions there are, and this one's windows with their panes laid out.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    /// The session attached to, by its number (`$3` is 3).
    pub session: Option<u32>,
    pub sessions: Vec<SessionInfo>,
    pub windows: Vec<Window>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: u32,
    pub name: String,
    pub windows: u32,
    /// How many clients are attached to it, this one included.
    pub attached: u32,
    /// Its windows, in order. Known only while attached to tmux: asked
    /// from outside, a session is its name and a count.
    #[serde(default)]
    pub list: Vec<WindowBrief>,
}

/// A window of any session, as the list of sessions shows it: enough to
/// name it, go to it, and find what is running in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowBrief {
    pub id: u32,
    pub index: u32,
    pub name: String,
    /// The window of its session that is in front.
    pub active: bool,
    pub panes: Vec<PaneBrief>,
}

/// A pane, as what is running in it is found: by the process tmux started
/// in it, and the terminal that process and its children are on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneBrief {
    pub id: u32,
    pub pid: u32,
    /// As the remote names it: `/dev/pts/3`.
    pub tty: String,
}

/// A tmux window: a tab, to the page.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub id: u32,
    pub index: u32,
    pub name: String,
    pub active: bool,
    /// One pane is filling it, and the rest are out of sight until it is not.
    pub zoomed: bool,
    pub cols: u16,
    pub rows: u16,
    /// The panes in sight, each where the layout puts it.
    pub panes: Vec<Pane>,
    /// How many panes it has, in sight or not.
    pub pane_count: usize,
    pub dividers: Vec<Divider>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pane {
    pub id: u32,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// The pane of its window that is typed into.
    pub active: bool,
    /// What is running in it, as tmux names it: `bash`, `make`, `vim`.
    pub command: String,
}

/// A line between two boxes of a layout, and how far it can be dragged
/// either way, in cells.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Divider {
    /// The way to it through the splits, then its place in the last: `1:0`.
    pub id: String,
    pub dir: Dir,
    pub x: u16,
    pub y: u16,
    pub length: u16,
    pub lo: i32,
    pub hi: i32,
}

/// What `list-sessions` is asked to print for each: the name last, being the
/// one field that can have anything in it.
pub const SESSIONS_FORMAT: &str =
    "#{session_id}\t#{session_windows}\t#{session_attached}\t#{session_name}";
const WINDOWS_FORMAT: &str = "#{session_id}\t#{window_id}\t#{window_index}\t#{window_active}\t\
     #{window_zoomed_flag}\t#{window_visible_layout}\t#{window_name}";
const PANES_FORMAT: &str =
    "#{pane_id}\t#{window_id}\t#{pane_active}\t#{pane_pid}\t#{pane_tty}\t#{pane_current_command}";
/// What a pane's terminal has to be told that the text on it does not say.
const MODES_FORMAT: &str = "#{cursor_x},#{cursor_y},#{alternate_on},#{cursor_flag},\
     #{keypad_cursor_flag},#{keypad_flag},#{mouse_standard_flag},#{mouse_button_flag},\
     #{mouse_any_flag},#{mouse_sgr_flag},#{scroll_region_upper},#{scroll_region_lower},\
     #{pane_height}";

/// An id as tmux writes one, less its sigil: `%12`, `@3` and `$0`.
fn id(text: &str, sigil: char) -> Option<u32> {
    text.strip_prefix(sigil)?.parse().ok()
}

/// One line of `list-sessions`, printed with [`SESSIONS_FORMAT`].
pub fn parse_session(line: &str) -> Option<SessionInfo> {
    let mut fields = line.splitn(4, '\t');
    Some(SessionInfo {
        id: id(fields.next()?, '$')?,
        windows: fields.next()?.parse().ok()?,
        attached: fields.next()?.parse().ok()?,
        name: fields.next()?.to_owned(),
        list: Vec::new(),
    })
}

/// A word for tmux's command line, quoted so that nothing in it is read as
/// anything but itself. Control characters go: a name is one line.
fn quoted(text: &str) -> String {
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    format!("'{}'", clean.replace('\'', r"'\''"))
}

/// The bytes of a pane's output, out of the form a notice carries them in:
/// anything below a space, and a backslash, is three octal digits after one.
pub fn unescape(data: &[u8], into: &mut Vec<u8>) {
    let mut i = 0;
    while i < data.len() {
        let octal = |at: usize| data.get(at).filter(|b| (b'0'..=b'7').contains(b));
        match (data[i], octal(i + 1), octal(i + 2), octal(i + 3)) {
            (b'\\', Some(a), Some(b), Some(c)) => {
                let value =
                    u32::from(a - b'0') * 64 + u32::from(b - b'0') * 8 + u32::from(c - b'0');
                into.push(value as u8);
                i += 4;
            }
            (byte, ..) => {
                into.push(byte);
                i += 1;
            }
        }
    }
}

// --- what is asked of it ----------------------------------------------------------

/// Something the page asks for. Ids are tmux's own numbers.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "do", rename_all = "camelCase")]
pub enum Action {
    /// The room there is for a window, in cells.
    Resize {
        cols: u16,
        rows: u16,
    },
    /// A terminal has been made for a pane: show it, and say what it prints.
    Watch {
        pane: u32,
    },
    Unwatch {
        pane: u32,
    },
    /// What was typed, as the terminal emulator encoded it.
    Write {
        pane: u32,
        text: String,
    },
    /// The same for what is not text: a mouse report in the old encoding.
    WriteBytes {
        pane: u32,
        bytes: Vec<u8>,
    },
    SelectPane {
        pane: u32,
    },
    Split {
        pane: u32,
        dir: Dir,
    },
    KillPane {
        pane: u32,
    },
    Zoom {
        pane: u32,
    },
    /// Bring a window to the front: of this session, or of the one named,
    /// which is then gone to.
    SelectWindow {
        window: u32,
        #[serde(default)]
        session: Option<u32>,
    },
    /// In this session, or in the one named, which is then gone to.
    NewWindow {
        #[serde(default)]
        session: Option<u32>,
    },
    KillWindow {
        window: u32,
    },
    RenameWindow {
        window: u32,
        name: String,
    },
    SwitchSession {
        session: u32,
    },
    /// With no name, tmux numbers it.
    NewSession {
        name: String,
    },
    KillSession {
        session: u32,
    },
    RenameSession {
        session: u32,
        name: String,
    },
    MoveDivider {
        window: u32,
        divider: String,
        delta: i32,
    },
    /// Put a window somewhere else in the order: before another window,
    /// whichever session that is in, or at the end of a session.
    MoveWindow {
        window: u32,
        #[serde(default)]
        before: Option<u32>,
        #[serde(default)]
        session: Option<u32>,
    },
    /// What was typed, for whichever pane is being shown: for an attachment
    /// that follows the one pane of its session and has no need of its number.
    Type {
        bytes: Vec<u8>,
    },
    /// End the session attached to, and everything running in it.
    End,
}

/// Where something new starts: the folder the pane it comes from is in,
/// rather than the one its session was started in. tmux fills this in.
const HERE: &str = "-c '#{pane_current_path}'";

/// What to do at the start: join a session that is there, or make one.
#[derive(Debug, Clone)]
pub enum Start {
    Attach(u32),
    New(String),
    /// One of the editor's own terminals, kept in a session of its name: the
    /// session if it is there, and a new one, started in `cwd`, if it is not.
    Kept {
        name: String,
        cwd: Option<String>,
    },
}

/// The socket the editor's terminals are kept on: a tmux server of their
/// own, so that none of them is ever among the sessions a person made.
pub const KEPT_SOCKET: &str = "tide";

/// A name for one of those sessions, with nothing in it but what a name
/// there can safely have.
pub fn kept_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

impl Start {
    /// The arguments to `tmux` that do it, for a shell to run.
    pub fn arguments(&self, cols: u16, rows: u16) -> String {
        match self {
            Start::Attach(session) => format!("-u -C attach-session -t '${session}'"),
            Start::New(name) => {
                let mut line = format!("-u -C new-session -x {} -y {}", cols.max(2), rows.max(2));
                let name: String = name.chars().filter(|c| !c.is_control()).collect();
                if !name.trim().is_empty() {
                    // Quoted for the shell, which is who reads this one.
                    line.push_str(&format!(" -s '{}'", name.trim().replace('\'', r"'\''")));
                }
                line
            }
            // The server first, so that it can be told two things before
            // there is a shell to be told them late: to say it draws more
            // than eight colours where its config has left that as it comes,
            // and that every colour there is can be drawn.
            Start::Kept { name, cwd } => {
                let mut line = format!(
                    "-L {KEPT_SOCKET} -u -C start-server \\; \
                     if-shell -F '#{{==:#{{default-terminal}},screen}}' \
                     'set-option -g default-terminal screen-256color' \\; \
                     set-environment -g COLORTERM truecolor \\; \
                     new-session -A -s '{}' -x {} -y {}",
                    kept_name(name),
                    cols.max(2),
                    rows.max(2)
                );
                if let Some(cwd) = cwd {
                    line.push_str(&format!(" -c '{}'", cwd.replace('\'', r"'\''")));
                }
                line
            }
        }
    }
}

/// What a remote has of tmux, asked before attaching to any of it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    /// Whether there is a `tmux` to run at all.
    pub installed: bool,
    pub sessions: Vec<SessionInfo>,
}

/// How an attachment ended.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ended {
    /// Whether it ever attached. If not, `reason` is why.
    pub attached: bool,
    /// Whether tmux said it was letting go. It does when a session ends or
    /// is detached from; a connection that dies under it says nothing.
    pub exited: bool,
    pub reason: Option<String>,
}

/// Something for the page.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// What a watched pane printed, or -- the first time, and after tmux
    /// has skipped ahead -- the whole of it as it stands.
    Output {
        pane: u32,
        data: Vec<u8>,
    },
    State(State),
    /// Panes have new sizes: each pane, its columns and its rows. Said the
    /// moment tmux says a layout changed, ahead of the full account of it:
    /// what a pane prints next is printed for the size it now has, and has
    /// to be drawn at it.
    Sizes(Vec<(u32, u16, u16)>),
    /// tmux refused something that was asked for, and said why.
    Error(String),
}

/// What an [`Engine`] wants done: bytes written to tmux, and things shown.
#[derive(Debug, Default)]
pub struct Out {
    pub commands: Vec<u8>,
    pub events: Vec<Event>,
}

// --- the engine -----------------------------------------------------------------

/// What a command that was sent is for, so its answer can be put to use.
/// tmux answers in the order it was asked.
enum Pending {
    /// Nothing hangs on the answer.
    Ignore,
    /// Something the user asked for: a refusal is shown.
    Report,
    Session,
    Sessions,
    Windows,
    Panes,
    Modes(u32),
    Capture(u32),
    /// A session was made, and is to be gone to.
    Created,
}

/// An answer being read: the lines between a `%begin` and its `%end`.
struct Block {
    time: Vec<u8>,
    number: Vec<u8>,
    /// Whether it answers something sent from here. The first does not: it
    /// is tmux's answer to being started.
    ours: bool,
    lines: Vec<Vec<u8>>,
}

/// A pane with a terminal on the page.
enum Watch {
    /// Waiting to be shown whole. Nothing it prints is passed on until then:
    /// it is already in what is about to be shown.
    Seeding {
        modes: Option<Modes>,
    },
    Live,
}

/// The state of a pane's terminal, as tmux has it.
#[derive(Debug, Default, PartialEq)]
struct Modes {
    cursor_x: u16,
    cursor_y: u16,
    alternate: bool,
    cursor_shown: bool,
    keypad_cursor: bool,
    keypad: bool,
    mouse_standard: bool,
    mouse_button: bool,
    mouse_any: bool,
    mouse_sgr: bool,
    scroll_top: u16,
    scroll_bottom: u16,
    height: u16,
}

impl Modes {
    fn parse(line: &str) -> Modes {
        let mut fields = line
            .split(',')
            .map(|field| field.parse::<u16>().unwrap_or(0));
        let mut next = || fields.next().unwrap_or(0);
        Modes {
            cursor_x: next(),
            cursor_y: next(),
            alternate: next() == 1,
            cursor_shown: next() == 1,
            keypad_cursor: next() == 1,
            keypad: next() == 1,
            mouse_standard: next() == 1,
            mouse_button: next() == 1,
            mouse_any: next() == 1,
            mouse_sgr: next() == 1,
            scroll_top: next(),
            scroll_bottom: next(),
            height: next(),
        }
    }
}

/// The answers to one asking of what there is, as they come in.
#[derive(Default)]
struct Refresh {
    session: Option<u32>,
    sessions: Vec<SessionInfo>,
    windows: Vec<String>,
    failed: bool,
}

/// One attachment to tmux: fed what it writes, and asked for what the page
/// wants of it.
pub struct Engine {
    cols: u16,
    rows: u16,
    /// How many lines of a pane's past are shown with it.
    history: u32,
    /// A line not yet ended.
    partial: Vec<u8>,
    block: Option<Block>,
    pending: VecDeque<Pending>,
    /// tmux has answered to being started, and is attached.
    started: bool,
    /// Why it would not start, or why it ended.
    reason: Option<String>,
    /// Something moved, and what there is has to be asked again.
    dirty: bool,
    refresh: Option<Refresh>,
    state: Option<State>,
    /// Each window's layout as last read, to move its lines in.
    layouts: HashMap<u32, Cell>,
    watched: HashMap<u32, Watch>,
    /// The panes there are that tmux has been told what to do about.
    known: HashSet<u32>,
    /// Show every pane of the session as soon as it is known of, unasked.
    follow: bool,
    /// tmux has said it is letting go.
    exited: bool,
}

impl Engine {
    pub fn new(cols: u16, rows: u16, history: u32) -> Self {
        Self {
            cols,
            rows,
            history,
            partial: Vec::new(),
            block: None,
            pending: VecDeque::new(),
            started: false,
            reason: None,
            dirty: false,
            refresh: None,
            state: None,
            layouts: HashMap::new(),
            watched: HashMap::new(),
            known: HashSet::new(),
            follow: false,
            exited: false,
        }
    }

    /// An attachment for one terminal: it shows the pane of its session
    /// without being asked to, and is typed at without the pane being named.
    pub fn following(cols: u16, rows: u16, history: u32) -> Self {
        Self {
            follow: true,
            ..Self::new(cols, rows, history)
        }
    }

    /// Whether tmux said it was letting go, as against simply going quiet.
    pub fn exited(&self) -> bool {
        self.exited
    }

    /// Whether tmux attached to a session at all.
    pub fn started(&self) -> bool {
        self.started
    }

    /// Why it would not start or why it ended, where it said.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Take in what tmux wrote.
    pub fn feed(&mut self, bytes: &[u8], out: &mut Out) {
        let mut rest = bytes;
        while let Some(end) = rest.iter().position(|byte| *byte == b'\n') {
            let (head, tail) = rest.split_at(end);
            rest = &tail[1..];
            if self.partial.is_empty() {
                self.line(head, out);
            } else {
                let mut line = std::mem::take(&mut self.partial);
                line.extend_from_slice(head);
                self.line(&line, out);
            }
        }
        self.partial.extend_from_slice(rest);
        self.settle(out);
    }

    /// Ask what there is, if something has moved and it is not being asked.
    fn settle(&mut self, out: &mut Out) {
        if !self.started || !self.dirty || self.refresh.is_some() {
            return;
        }
        self.dirty = false;
        self.refresh = Some(Refresh::default());
        self.send(out, "display-message -p '#{session_id}'", Pending::Session);
        self.send(
            out,
            &format!("list-sessions -F '{SESSIONS_FORMAT}'"),
            Pending::Sessions,
        );
        self.send(
            out,
            &format!("list-windows -a -F '{WINDOWS_FORMAT}'"),
            Pending::Windows,
        );
        self.send(
            out,
            &format!("list-panes -a -F '{PANES_FORMAT}'"),
            Pending::Panes,
        );
    }

    fn send(&mut self, out: &mut Out, command: &str, pending: Pending) {
        out.commands.extend_from_slice(command.as_bytes());
        out.commands.push(b'\n');
        self.pending.push_back(pending);
    }

    fn line(&mut self, line: &[u8], out: &mut Out) {
        // A line of an answer, unless it is the one that ends it.
        if let Some(block) = &mut self.block {
            let ended = [(&b"%end "[..], true), (&b"%error "[..], false)]
                .into_iter()
                .find_map(|(word, ok)| {
                    let mut fields = line.strip_prefix(word)?.split(|byte| *byte == b' ');
                    (fields.next()? == block.time && fields.next()? == block.number).then_some(ok)
                });
            match ended {
                Some(ok) => {
                    let block = self.block.take().expect("just matched on it");
                    self.answered(block, ok, out);
                }
                None => block.lines.push(line.to_vec()),
            }
            return;
        }
        let Some(notice) = line.strip_prefix(b"%") else {
            // Whatever a login shell said on its way to starting tmux.
            return;
        };
        let (word, rest) = match notice.iter().position(|byte| *byte == b' ') {
            Some(space) => (&notice[..space], &notice[space + 1..]),
            None => (notice, &b""[..]),
        };
        match word {
            b"begin" => {
                let mut fields = rest.split(|byte| *byte == b' ');
                let (time, number) = (fields.next(), fields.next());
                self.block = Some(Block {
                    time: time.unwrap_or_default().to_vec(),
                    number: number.unwrap_or_default().to_vec(),
                    ours: fields.next() == Some(b"1"),
                    lines: Vec::new(),
                });
            }
            b"output" => {
                if let Some(space) = rest.iter().position(|byte| *byte == b' ') {
                    self.printed(&rest[..space], &rest[space + 1..], out);
                }
            }
            // The same, with how late it is: `%pane age ... : data`.
            b"extended-output" => {
                let pane = rest.split(|byte| *byte == b' ').next().unwrap_or_default();
                if let Some(colon) = rest.windows(3).position(|at| at == b" : ") {
                    self.printed(pane, &rest[colon + 3..], out);
                }
            }
            // It fell too far behind, and tmux has stopped sending it: carry
            // on from where the pane is now, shown whole.
            b"pause" => {
                let pane = std::str::from_utf8(rest)
                    .ok()
                    .and_then(|pane| id(pane, '%'));
                if let Some(pane) = pane.filter(|pane| self.watched.contains_key(pane)) {
                    self.send(
                        out,
                        &format!("refresh-client -A '%{pane}:continue'"),
                        Pending::Ignore,
                    );
                    self.seed(pane, out);
                }
            }
            b"exit" => {
                self.exited = true;
                if !rest.is_empty() {
                    self.reason = Some(String::from_utf8_lossy(rest).into_owned());
                }
            }
            // `@window layout visible-layout flags`: what is in sight is the
            // second of the two, where there is one.
            b"layout-change" => {
                let mut fields = rest.split(|byte| *byte == b' ').skip(1);
                let (whole, visible) = (fields.next(), fields.next());
                let layout = visible
                    .or(whole)
                    .and_then(|layout| std::str::from_utf8(layout).ok())
                    .and_then(Cell::parse);
                if let Some(layout) = layout {
                    let mut boxes = Vec::new();
                    layout.panes(&mut boxes);
                    let sizes: Vec<_> = boxes
                        .into_iter()
                        .filter(|(pane, _)| self.watched.contains_key(pane))
                        .map(|(pane, cell)| (pane, cell.w, cell.h))
                        .collect();
                    if !sizes.is_empty() {
                        out.events.push(Event::Sizes(sizes));
                    }
                }
                self.dirty = true;
            }
            b"window-add"
            | b"window-close"
            | b"window-renamed"
            | b"window-pane-changed"
            | b"session-changed"
            | b"session-renamed"
            | b"sessions-changed"
            | b"session-window-changed"
            | b"unlinked-window-add"
            | b"unlinked-window-close"
            | b"unlinked-window-renamed"
            | b"client-session-changed"
            | b"client-detached" => self.dirty = true,
            _ => {}
        }
        // Attached, by any sign of it, should the answer to starting be missed.
        if !self.started && self.reason.is_none() && word != b"begin" && word != b"exit" {
            self.start(out);
        }
    }

    fn start(&mut self, out: &mut Out) {
        self.started = true;
        self.dirty = true;
        let size = format!("refresh-client -C {}x{}", self.cols, self.rows);
        self.send(out, &size, Pending::Ignore);
        // Refused by a tmux too old to have it, which then never skips ahead.
        self.send(
            out,
            &format!("refresh-client -f pause-after={PAUSE_AFTER}"),
            Pending::Ignore,
        );
    }

    /// A pane printed something.
    fn printed(&mut self, pane: &[u8], data: &[u8], out: &mut Out) {
        let Some(pane) = std::str::from_utf8(pane)
            .ok()
            .and_then(|pane| id(pane, '%'))
        else {
            return;
        };
        if !matches!(self.watched.get(&pane), Some(Watch::Live)) {
            return;
        }
        // One piece for a run from the same pane: a build prints a line at a
        // time, and each would otherwise be a message to the page.
        if let Some(Event::Output {
            pane: last,
            data: so_far,
        }) = out.events.last_mut()
        {
            if *last == pane {
                unescape(data, so_far);
                return;
            }
        }
        let mut bytes = Vec::with_capacity(data.len());
        unescape(data, &mut bytes);
        out.events.push(Event::Output { pane, data: bytes });
    }

    /// An answer has ended.
    fn answered(&mut self, block: Block, ok: bool, out: &mut Out) {
        let text = |lines: &[Vec<u8>]| {
            lines
                .iter()
                .map(|line| String::from_utf8_lossy(line))
                .collect::<Vec<_>>()
                .join("\n")
        };
        if !block.ours {
            // tmux's answer to being started: attached, or why not.
            if !self.started && self.reason.is_none() {
                match ok {
                    true => self.start(out),
                    false => self.reason = Some(text(&block.lines)),
                }
            }
            return;
        }
        let Some(pending) = self.pending.pop_front() else {
            return;
        };
        let first = block
            .lines
            .first()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .unwrap_or_default();

        match pending {
            Pending::Ignore => {}
            Pending::Report => {
                if !ok {
                    out.events.push(Event::Error(text(&block.lines)));
                }
            }
            Pending::Created => match (ok, id(first.trim(), '$')) {
                (true, Some(session)) => {
                    let go = format!("switch-client -t '${session}'");
                    self.send(out, &go, Pending::Report);
                }
                (true, None) => {}
                (false, _) => out.events.push(Event::Error(text(&block.lines))),
            },
            Pending::Session => {
                if let Some(refresh) = &mut self.refresh {
                    refresh.session = id(first.trim(), '$');
                    refresh.failed |= !ok;
                }
            }
            Pending::Sessions => {
                if let Some(refresh) = &mut self.refresh {
                    refresh.sessions = text(&block.lines)
                        .lines()
                        .filter_map(parse_session)
                        .collect();
                    refresh.failed |= !ok;
                }
            }
            Pending::Windows => {
                if let Some(refresh) = &mut self.refresh {
                    refresh.windows = text(&block.lines).lines().map(str::to_owned).collect();
                    refresh.failed |= !ok;
                }
            }
            Pending::Panes => {
                if let Some(refresh) = self.refresh.take() {
                    match ok && !refresh.failed {
                        true => self.assemble(refresh, &text(&block.lines), out),
                        // Something went from under it part way; ask again.
                        false => self.dirty = true,
                    }
                }
            }
            Pending::Modes(pane) => match (ok, self.watched.get_mut(&pane)) {
                (true, Some(Watch::Seeding { modes })) => *modes = Some(Modes::parse(first.trim())),
                (false, Some(_)) => drop(self.watched.remove(&pane)),
                _ => {}
            },
            Pending::Capture(pane) => match (ok, self.watched.get_mut(&pane)) {
                (true, Some(Watch::Seeding { modes })) => {
                    let modes = modes.take().unwrap_or_default();
                    out.events.push(Event::Output {
                        pane,
                        data: seed(&block.lines, &modes),
                    });
                    self.watched.insert(pane, Watch::Live);
                }
                (false, Some(_)) => drop(self.watched.remove(&pane)),
                _ => {}
            },
        }
    }

    /// Put the answers together into what there is, and say so if it is not
    /// what there was.
    fn assemble(&mut self, refresh: Refresh, panes: &str, out: &mut Out) {
        struct Info {
            window: u32,
            active: bool,
            pid: u32,
            tty: String,
            command: String,
        }
        // Every pane there is, whichever session it is in.
        let mut info: HashMap<u32, Info> = HashMap::new();
        for line in panes.lines() {
            let mut fields = line.splitn(6, '\t');
            let parsed = (|| {
                let pane = id(fields.next()?, '%')?;
                let window = id(fields.next()?, '@')?;
                let active = fields.next()? == "1";
                let pid = fields.next()?.parse().unwrap_or(0);
                let tty = fields.next()?.to_owned();
                let command = fields.next()?.to_owned();
                Some((
                    pane,
                    Info {
                        window,
                        active,
                        pid,
                        tty,
                        command,
                    },
                ))
            })();
            if let Some((pane, each)) = parsed {
                info.insert(pane, each);
            }
        }
        let mut panes_of: HashMap<u32, Vec<PaneBrief>> = HashMap::new();
        for (pane, each) in &info {
            panes_of.entry(each.window).or_default().push(PaneBrief {
                id: *pane,
                pid: each.pid,
                tty: each.tty.clone(),
            });
        }
        panes_of
            .values_mut()
            .for_each(|panes| panes.sort_unstable_by_key(|pane| pane.id));

        self.layouts.clear();
        let mut windows = Vec::new();
        let mut sessions = refresh.sessions;
        // The panes of the session attached to: the ones tmux sends the
        // output of, and so the ones to tell it about.
        let mut ours: HashSet<u32> = HashSet::new();
        for line in &refresh.windows {
            let mut fields = line.splitn(7, '\t');
            let parsed = (|| {
                let session = id(fields.next()?, '$')?;
                let window = id(fields.next()?, '@')?;
                let index = fields.next()?.parse().ok()?;
                let active = fields.next()? == "1";
                let zoomed = fields.next()? == "1";
                let layout = Cell::parse(fields.next()?)?;
                let name = fields.next()?.to_owned();
                Some((session, window, index, active, zoomed, layout, name))
            })();
            let Some((session, window, index, active, zoomed, layout, name)) = parsed else {
                continue;
            };
            // Every window is listed under its session; only the windows of
            // the session attached to are laid out.
            if let Some(owner) = sessions.iter_mut().find(|each| each.id == session) {
                owner.list.push(WindowBrief {
                    id: window,
                    index,
                    name: name.clone(),
                    active,
                    panes: panes_of.get(&window).cloned().unwrap_or_default(),
                });
            }
            if Some(session) != refresh.session {
                continue;
            }
            ours.extend(
                info.iter()
                    .filter(|(_, each)| each.window == window)
                    .map(|(pane, _)| *pane),
            );

            let mut boxes = Vec::new();
            layout.panes(&mut boxes);
            let panes = boxes
                .into_iter()
                .map(|(pane, cell)| Pane {
                    id: pane,
                    x: cell.x,
                    y: cell.y,
                    w: cell.w,
                    h: cell.h,
                    active: info.get(&pane).is_some_and(|each| each.active),
                    command: info
                        .get(&pane)
                        .map(|each| each.command.clone())
                        .unwrap_or_default(),
                })
                .collect();
            let mut dividers = Vec::new();
            // What is in sight of a zoomed window is not its layout.
            if !zoomed {
                layout.dividers(&mut Vec::new(), &mut dividers);
            }
            windows.push(Window {
                id: window,
                index,
                name,
                active,
                zoomed,
                cols: layout.w,
                rows: layout.h,
                panes,
                pane_count: info.values().filter(|each| each.window == window).count(),
                dividers,
            });
            if !zoomed {
                self.layouts.insert(window, layout);
            }
        }

        // A pane that has gone needs no terminal, and one that is new prints
        // to no one until the page has a terminal for it.
        self.watched.retain(|pane, _| info.contains_key(pane));
        self.known.retain(|pane| ours.contains(pane));
        let mut fresh: Vec<u32> = ours
            .iter()
            .filter(|pane| !self.known.contains(pane))
            .copied()
            .collect();
        fresh.sort_unstable();
        for pane in fresh {
            self.known.insert(pane);
            if self.follow {
                self.seed(pane, out);
            } else if !self.watched.contains_key(&pane) {
                self.send(
                    out,
                    &format!("refresh-client -A '%{pane}:off'"),
                    Pending::Ignore,
                );
            }
        }

        let state = State {
            session: refresh.session,
            sessions,
            windows,
        };
        if self.state.as_ref() != Some(&state) {
            self.state = Some(state.clone());
            out.events.push(Event::State(state));
        }
    }

    /// Show a pane whole: how its terminal stands, then what is on it.
    fn seed(&mut self, pane: u32, out: &mut Out) {
        self.watched.insert(pane, Watch::Seeding { modes: None });
        self.send(
            out,
            &format!("display-message -p -t %{pane} '{MODES_FORMAT}'"),
            Pending::Modes(pane),
        );
        // With its colours, its trailing spaces -- a status line is mostly
        // those -- and as much of its past as the page keeps.
        self.send(
            out,
            &format!("capture-pane -p -e -N -S -{} -t %{pane}", self.history),
            Pending::Capture(pane),
        );
    }

    /// Do something the page asked for.
    pub fn act(&mut self, action: Action, out: &mut Out) {
        // Not attached, there is no one to ask; the size is kept for when it is.
        if let Action::Resize { cols, rows } = action {
            if (cols, rows) == (self.cols, self.rows) || cols < 2 || rows < 2 {
                return;
            }
            (self.cols, self.rows) = (cols, rows);
        }
        if !self.started {
            return;
        }
        match action {
            Action::Resize { cols, rows } => {
                self.send(
                    out,
                    &format!("refresh-client -C {cols}x{rows}"),
                    Pending::Ignore,
                );
            }
            Action::Watch { pane } => {
                self.known.insert(pane);
                self.send(
                    out,
                    &format!("refresh-client -A '%{pane}:on'"),
                    Pending::Ignore,
                );
                self.seed(pane, out);
            }
            Action::Unwatch { pane } => {
                if self.watched.remove(&pane).is_some() {
                    self.send(
                        out,
                        &format!("refresh-client -A '%{pane}:off'"),
                        Pending::Ignore,
                    );
                }
            }
            Action::Type { bytes } => {
                let mut panes: Vec<u32> = self.watched.keys().copied().collect();
                panes.sort_unstable();
                for pane in panes {
                    self.keys(pane, &bytes, out);
                }
            }
            Action::End => self.send(out, "kill-session", Pending::Ignore),
            Action::Write { pane, text } => self.keys(pane, text.as_bytes(), out),
            Action::WriteBytes { pane, bytes } => self.keys(pane, &bytes, out),
            Action::SelectPane { pane } => {
                self.send(out, &format!("select-pane -t %{pane}"), Pending::Ignore);
            }
            Action::Split { pane, dir } => {
                let flag = match dir {
                    Dir::Row => "-h",
                    Dir::Column => "-v",
                };
                self.send(
                    out,
                    &format!("split-window {flag} {HERE} -t %{pane}"),
                    Pending::Report,
                );
            }
            Action::KillPane { pane } => {
                self.send(out, &format!("kill-pane -t %{pane}"), Pending::Report);
            }
            Action::Zoom { pane } => {
                self.send(out, &format!("resize-pane -Z -t %{pane}"), Pending::Report);
            }
            Action::SelectWindow { window, session } => {
                self.send(out, &format!("select-window -t @{window}"), Pending::Ignore);
                self.go_to(session, out);
            }
            Action::NewWindow { session } => {
                match session {
                    Some(session) => {
                        let command = format!("new-window {HERE} -t '${session}:'");
                        self.send(out, &command, Pending::Report);
                    }
                    None => self.send(out, &format!("new-window {HERE}"), Pending::Report),
                }
                self.go_to(session, out);
            }
            Action::KillWindow { window } => {
                self.send(out, &format!("kill-window -t @{window}"), Pending::Report);
            }
            Action::RenameWindow { window, name } => {
                let command = format!("rename-window -t @{window} -- {}", quoted(&name));
                self.send(out, &command, Pending::Report);
            }
            Action::SwitchSession { session } => {
                self.send(
                    out,
                    &format!("switch-client -t '${session}'"),
                    Pending::Report,
                );
            }
            Action::NewSession { name } => {
                let mut command = format!(
                    "new-session -d -P -F '#{{session_id}}' -x {} -y {}",
                    self.cols, self.rows
                );
                if !name.trim().is_empty() {
                    command.push_str(&format!(" -s {}", quoted(name.trim())));
                }
                self.send(out, &command, Pending::Created);
            }
            Action::KillSession { session } => {
                // tmux lets go of a client whose session ends. With another
                // session to go to, this goes there first and stays attached.
                let current = self.state.as_ref().and_then(|state| state.session);
                let other = self.state.as_ref().and_then(|state| {
                    state
                        .sessions
                        .iter()
                        .find(|each| each.id != session)
                        .map(|each| each.id)
                });
                if let (true, Some(other)) = (current == Some(session), other) {
                    self.send(
                        out,
                        &format!("switch-client -t '${other}'"),
                        Pending::Ignore,
                    );
                }
                self.send(
                    out,
                    &format!("kill-session -t '${session}'"),
                    Pending::Report,
                );
            }
            Action::RenameSession { session, name } => {
                let command = format!("rename-session -t '${session}' -- {}", quoted(name.trim()));
                self.send(out, &command, Pending::Report);
            }
            // Without going to it: putting a window in order is not asking
            // to look at it.
            Action::MoveWindow {
                window,
                before,
                session,
            } => match (before, session) {
                (Some(before), _) if before != window => {
                    let command = format!("move-window -d -b -s @{window} -t @{before}");
                    self.send(out, &command, Pending::Report);
                }
                (None, Some(session)) => {
                    let command = format!("move-window -d -s @{window} -t '${session}:'");
                    self.send(out, &command, Pending::Report);
                }
                _ => {}
            },
            Action::MoveDivider {
                window,
                divider,
                delta,
            } => {
                let Some(layout) = self.layouts.get_mut(&window) else {
                    return;
                };
                if layout.move_divider(&divider, delta) {
                    let command = format!("select-layout -t @{window} '{}'", layout.serialize());
                    self.send(out, &command, Pending::Report);
                }
            }
        }
    }

    /// Go to a session, if one is named and it is not the one attached to.
    fn go_to(&mut self, session: Option<u32>, out: &mut Out) {
        let current = self.state.as_ref().and_then(|state| state.session);
        if let Some(session) = session.filter(|session| Some(*session) != current) {
            let command = format!("switch-client -t '${session}'");
            self.send(out, &command, Pending::Report);
        }
    }

    /// Type bytes at a pane, exactly as they are.
    fn keys(&mut self, pane: u32, bytes: &[u8], out: &mut Out) {
        use std::fmt::Write;
        for chunk in bytes.chunks(KEYS_PER_COMMAND) {
            let mut command = format!("send-keys -t %{pane} -H");
            for byte in chunk {
                let _ = write!(command, " {byte:02x}");
            }
            self.send(out, &command, Pending::Ignore);
        }
    }
}

/// What to write to a blank terminal to make it the pane tmux has: the lines
/// of the pane, its past first, and then the state the text does not carry.
fn seed(lines: &[Vec<u8>], modes: &Modes) -> Vec<u8> {
    // From nothing: this is also what follows tmux skipping ahead.
    let mut data = b"\x1bc".to_vec();
    if modes.alternate {
        data.extend_from_slice(b"\x1b[?1049h");
    }
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            data.extend_from_slice(b"\r\n");
        }
        data.extend_from_slice(line);
    }
    data.extend_from_slice(b"\x1b[0m");
    // Setting the region sends the cursor home, so the cursor comes after.
    if modes.height > 0 && (modes.scroll_top, modes.scroll_bottom + 1) != (0, modes.height) {
        let region = format!("\x1b[{};{}r", modes.scroll_top + 1, modes.scroll_bottom + 1);
        data.extend_from_slice(region.as_bytes());
    }
    let cursor = format!("\x1b[{};{}H", modes.cursor_y + 1, modes.cursor_x + 1);
    data.extend_from_slice(cursor.as_bytes());

    let set: [(bool, &[u8]); 7] = [
        (!modes.cursor_shown, b"\x1b[?25l"),
        (modes.keypad_cursor, b"\x1b[?1h"),
        (modes.keypad, b"\x1b="),
        (modes.mouse_standard, b"\x1b[?1000h"),
        (modes.mouse_button, b"\x1b[?1002h"),
        (modes.mouse_any, b"\x1b[?1003h"),
        (modes.mouse_sgr, b"\x1b[?1006h"),
    ];
    for (on, sequence) in set {
        if on {
            data.extend_from_slice(sequence);
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(out: &Out) -> Vec<&str> {
        std::str::from_utf8(&out.commands)
            .unwrap()
            .lines()
            .collect()
    }

    /// An engine attached to a session, with what it asked on the way taken.
    fn attached() -> Engine {
        let mut engine = Engine::new(100, 30, 2000);
        let mut out = Out::default();
        engine.feed(b"%begin 1 272 0\n%end 1 272 0\n", &mut out);
        assert!(engine.started());
        assert_eq!(
            &said(&out)[..2],
            [
                "refresh-client -C 100x30",
                "refresh-client -f pause-after=5"
            ]
        );
        engine
    }

    /// Answer everything the engine has asked that is not answered yet.
    fn answer(engine: &mut Engine, answers: &[&str]) -> Out {
        let mut out = Out::default();
        for (number, body) in answers.iter().enumerate() {
            let block = format!("%begin 5 {number} 1\n{body}%end 5 {number} 1\n");
            engine.feed(block.as_bytes(), &mut out);
        }
        out
    }

    const TWO_PANES: &str = "6b8b,100x30,0,0{50x30,0,0,0,49x30,51,0,1}";
    const THREE_PANES: &str = "77dd,100x30,0,0{50x30,0,0,0,49x30,51,0[49x15,51,0,1,49x14,51,16,2]}";

    /// A window of the session that is not attached to, and every pane.
    const OTHER_WINDOW: &str = "$1\t@5\t0\t1\t0\ta880,100x30,0,0,3\tvim\n";
    const PANES: &str = "%0\t@0\t0\t100\t/dev/pts/1\tbash\n%1\t@0\t1\t101\t/dev/pts/2\tmake\n\
                         %2\t@0\t0\t102\t/dev/pts/3\tbash\n%3\t@5\t1\t400\t/dev/pts/9\tvim\n";

    /// Answer the two that open an attachment and the four of a refresh.
    fn refreshed(engine: &mut Engine, layout: &str) -> Out {
        let windows = format!("$0\t@0\t0\t1\t0\t{layout}\tbash\n{OTHER_WINDOW}");
        answer(
            engine,
            &[
                "",
                "",
                "$0\n",
                "$0\t1\t1\tfirst\n$1\t1\t0\tsecond\n",
                &windows,
                PANES,
            ],
        )
    }

    #[test]
    fn a_layout_is_read_and_written_back_as_tmux_writes_it() {
        for layout in [TWO_PANES, THREE_PANES, "a880,100x30,0,0,3"] {
            let cell = Cell::parse(layout).expect(layout);
            assert_eq!(cell.serialize(), layout);
        }
        assert!(Cell::parse("77dd,100x30,0,0{50x30").is_none());
        assert!(Cell::parse("nonsense").is_none());
    }

    #[test]
    fn a_line_is_dragged_as_far_as_the_panes_beside_it_allow() {
        let mut layout = Cell::parse(THREE_PANES).unwrap();
        // Between the left pane and the stack on its right.
        assert!(layout.move_divider(":0", -10));
        assert_eq!(
            layout.serialize().split_once(',').unwrap().1,
            "100x30,0,0{40x30,0,0,0,59x30,41,0[59x15,41,0,1,59x14,41,16,2]}"
        );
        // Between the two of the stack: the lower one keeps a row.
        assert!(layout.move_divider("1:0", 99));
        assert_eq!(
            layout.serialize().split_once(',').unwrap().1,
            "100x30,0,0{40x30,0,0,0,59x30,41,0[59x28,41,0,1,59x1,41,29,2]}"
        );
        assert!(!layout.move_divider("1:0", 1), "nothing left to give");
        assert!(!layout.move_divider("1:1", 1), "no such line");
        assert!(!layout.move_divider("7:0", 1), "no such split");
    }

    #[test]
    fn output_is_taken_out_of_its_escapes() {
        let mut bytes = Vec::new();
        unescape(br"\033[1mhi\015\012a\134b \0end", &mut bytes);
        assert_eq!(bytes, b"\x1b[1mhi\r\na\\b \\0end");
    }

    #[test]
    fn a_session_that_is_not_there_is_a_reason_and_not_a_start() {
        let mut engine = Engine::new(80, 24, 100);
        let mut out = Out::default();
        engine.feed(
            b"Last login: today\n%begin 1 272 0\nno sessions\n%error 1 272 0\n%exit\n",
            &mut out,
        );
        assert!(!engine.started());
        assert_eq!(engine.reason(), Some("no sessions"));
        assert!(out.commands.is_empty());
    }

    #[test]
    fn what_there_is_is_asked_once_attached_and_said_once_answered() {
        let mut engine = attached();
        let out = refreshed(&mut engine, THREE_PANES);

        let Some(Event::State(state)) = out.events.first() else {
            panic!("no state: {:?}", out.events);
        };
        assert_eq!(state.session, Some(0));
        assert_eq!(state.sessions[1].name, "second");
        // Every session's windows are listed under it, with where to look
        // for what runs in each; only this session's are laid out.
        assert_eq!(state.windows.len(), 1);
        assert_eq!(state.sessions[0].list[0].panes.len(), 3);
        let other = &state.sessions[1].list[0];
        assert_eq!((other.id, other.name.as_str()), (5, "vim"));
        assert_eq!(
            other.panes,
            [PaneBrief {
                id: 3,
                pid: 400,
                tty: "/dev/pts/9".into()
            }]
        );
        let window = &state.windows[0];
        assert_eq!((window.cols, window.rows, window.pane_count), (100, 30, 3));
        assert_eq!(window.panes[1].command, "make");
        assert!(window.panes[1].active);
        assert_eq!((window.panes[2].x, window.panes[2].y), (51, 16));
        assert_eq!(
            window
                .dividers
                .iter()
                .map(|line| line.id.as_str())
                .collect::<Vec<_>>(),
            [":0", "1:0"]
        );
        assert_eq!(
            (
                window.dividers[0].x,
                window.dividers[0].lo,
                window.dividers[0].hi
            ),
            (50, -49, 48)
        );
        // No terminal for any of them yet, so none is to send what it prints.
        assert_eq!(
            said(&out),
            [
                "refresh-client -A '%0:off'",
                "refresh-client -A '%1:off'",
                "refresh-client -A '%2:off'"
            ]
        );
    }

    #[test]
    fn a_notice_asks_again_and_the_same_answer_is_not_said_twice() {
        let mut engine = attached();
        refreshed(&mut engine, TWO_PANES);

        let mut out = Out::default();
        engine.feed(b"%window-renamed @0 bash\n", &mut out);
        assert_eq!(said(&out).len(), 4, "asked again");
        let windows = format!("$0\t@0\t0\t1\t0\t{TWO_PANES}\tbash\n{OTHER_WINDOW}");
        let out = answer(
            &mut engine,
            &[
                // The three that told the new panes to keep quiet.
                "",
                "",
                "",
                "$0\n",
                "$0\t1\t1\tfirst\n$1\t1\t0\tsecond\n",
                &windows,
                PANES,
            ],
        );
        assert!(
            out.events.is_empty(),
            "nothing had changed: {:?}",
            out.events
        );
    }

    #[test]
    fn a_watched_pane_is_shown_whole_and_then_as_it_prints() {
        let mut engine = attached();
        refreshed(&mut engine, TWO_PANES);
        answer(&mut engine, &["", "", ""]);

        let mut out = Out::default();
        engine.act(Action::Watch { pane: 1 }, &mut out);
        assert_eq!(said(&out)[0], "refresh-client -A '%1:on'");
        assert!(said(&out)[2].starts_with("capture-pane -p -e -N -S -2000 -t %1"));

        // What it prints meanwhile is in the capture already.
        let mut out = Out::default();
        engine.feed(b"%output %1 early\n", &mut out);
        assert!(out.events.is_empty());

        let out = answer(
            &mut engine,
            &["", "4,1,0,1,1,0,0,0,0,1,0,29,30\n", "one\ntwo  \n"],
        );
        assert_eq!(
            out.events,
            [Event::Output {
                pane: 1,
                data: b"\x1bcone\r\ntwo  \x1b[0m\x1b[2;5H\x1b[?1h\x1b[?1006h".to_vec()
            }]
        );

        // Then as it comes, a run of it as one piece; the others stay quiet.
        let mut out = Out::default();
        engine.feed(
            b"%output %1 a\\015\\012\n%extended-output %1 12 : b\n%output %0 no\n",
            &mut out,
        );
        assert_eq!(
            out.events,
            [Event::Output {
                pane: 1,
                data: b"a\r\nb".to_vec()
            }]
        );
    }

    #[test]
    fn a_new_size_is_said_ahead_of_what_is_printed_for_it() {
        let mut engine = attached();
        refreshed(&mut engine, TWO_PANES);
        let mut out = Out::default();
        engine.act(Action::Watch { pane: 1 }, &mut out);
        answer(
            &mut engine,
            &["", "", "", "", "0,0,0,1,0,0,0,0,0,0,0,29,30\n", "x\n"],
        );

        let mut out = Out::default();
        let notice =
            format!("%layout-change @0 {THREE_PANES} {THREE_PANES} *\n%output %1 redrawn\n");
        engine.feed(notice.as_bytes(), &mut out);
        // Only the pane with a terminal, and before what it then printed.
        assert_eq!(
            out.events,
            [
                Event::Sizes(vec![(1, 49, 15)]),
                Event::Output {
                    pane: 1,
                    data: b"redrawn".to_vec()
                }
            ]
        );
    }

    #[test]
    fn a_pane_tmux_skipped_ahead_in_is_shown_whole_again() {
        let mut engine = attached();
        refreshed(&mut engine, TWO_PANES);
        let mut out = Out::default();
        engine.act(Action::Watch { pane: 0 }, &mut out);
        answer(
            &mut engine,
            &["", "", "", "", "0,0,0,1,0,0,0,0,0,0,0,29,30\n", "x\n"],
        );

        let mut out = Out::default();
        engine.feed(b"%pause %0\n%output %0 lost\n", &mut out);
        assert_eq!(said(&out)[0], "refresh-client -A '%0:continue'");
        assert!(said(&out)[2].starts_with("capture-pane"));
        assert!(out.events.is_empty());
    }

    #[test]
    fn typing_goes_as_the_bytes_it_is() {
        let mut engine = attached();
        let mut out = Out::default();
        engine.act(
            Action::Write {
                pane: 3,
                text: "h\u{e9}\r".into(),
            },
            &mut out,
        );
        assert_eq!(said(&out), ["send-keys -t %3 -H 68 c3 a9 0d"]);

        let mut out = Out::default();
        engine.act(
            Action::Write {
                pane: 3,
                text: "x".repeat(450),
            },
            &mut out,
        );
        assert_eq!(said(&out).len(), 3, "a paste goes in pieces");
    }

    #[test]
    fn names_are_quoted_and_a_refusal_is_shown() {
        let mut engine = attached();
        refreshed(&mut engine, TWO_PANES);
        answer(&mut engine, &["", "", ""]);

        let mut out = Out::default();
        engine.act(
            Action::RenameWindow {
                window: 0,
                name: "it's; a\nname".into(),
            },
            &mut out,
        );
        assert_eq!(said(&out), [r"rename-window -t @0 -- 'it'\''s; aname'"]);

        let mut out = Out::default();
        engine.feed(
            b"%begin 9 1 1\nduplicate session: first\n%error 9 1 1\n",
            &mut out,
        );
        assert_eq!(
            out.events,
            [Event::Error("duplicate session: first".into())]
        );
    }

    #[test]
    fn a_new_session_is_gone_to_and_the_one_being_left_is_left_first() {
        let mut engine = attached();
        refreshed(&mut engine, TWO_PANES);
        answer(&mut engine, &["", "", ""]);

        let mut out = Out::default();
        engine.act(
            Action::NewSession {
                name: " build ".into(),
            },
            &mut out,
        );
        assert_eq!(
            said(&out),
            ["new-session -d -P -F '#{session_id}' -x 100 -y 30 -s 'build'"]
        );
        let out = answer(&mut engine, &["$4\n"]);
        assert_eq!(said(&out), ["switch-client -t '$4'"]);

        let mut out = Out::default();
        engine.act(Action::KillSession { session: 0 }, &mut out);
        assert_eq!(
            said(&out),
            ["switch-client -t '$1'", "kill-session -t '$0'"]
        );
        let mut out = Out::default();
        engine.act(Action::KillSession { session: 1 }, &mut out);
        assert_eq!(said(&out), ["kill-session -t '$1'"]);
    }

    #[test]
    fn a_window_of_another_session_is_gone_to_with_its_session() {
        let mut engine = attached();
        refreshed(&mut engine, TWO_PANES);
        answer(&mut engine, &["", "", ""]);

        let mut out = Out::default();
        let here = Action::SelectWindow {
            window: 0,
            session: Some(0),
        };
        engine.act(here, &mut out);
        assert_eq!(said(&out), ["select-window -t @0"]);

        let mut out = Out::default();
        let there = Action::SelectWindow {
            window: 5,
            session: Some(1),
        };
        engine.act(there, &mut out);
        assert_eq!(said(&out), ["select-window -t @5", "switch-client -t '$1'"]);

        let mut out = Out::default();
        engine.act(Action::NewWindow { session: Some(1) }, &mut out);
        assert_eq!(
            said(&out),
            [
                "new-window -c '#{pane_current_path}' -t '$1:'",
                "switch-client -t '$1'"
            ]
        );
        let mut out = Out::default();
        engine.act(Action::NewWindow { session: None }, &mut out);
        assert_eq!(said(&out), ["new-window -c '#{pane_current_path}'"]);
    }

    #[test]
    fn a_window_is_moved_before_another_or_to_the_end_of_a_session() {
        let mut engine = attached();
        let moved = |engine: &mut Engine, before, session| {
            let mut out = Out::default();
            let action = Action::MoveWindow {
                window: 4,
                before,
                session,
            };
            engine.act(action, &mut out);
            said(&out)
                .iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            moved(&mut engine, Some(2), Some(1)),
            ["move-window -d -b -s @4 -t @2"]
        );
        assert_eq!(
            moved(&mut engine, None, Some(1)),
            ["move-window -d -s @4 -t '$1:'"]
        );
        assert!(
            moved(&mut engine, Some(4), None).is_empty(),
            "before itself is where it is"
        );
    }

    #[test]
    fn a_dragged_line_is_a_layout_for_tmux_to_take() {
        let mut engine = attached();
        refreshed(&mut engine, TWO_PANES);
        answer(&mut engine, &["", "", ""]);

        let mut out = Out::default();
        engine.act(
            Action::MoveDivider {
                window: 0,
                divider: ":0".into(),
                delta: 5,
            },
            &mut out,
        );
        let layout = Cell::parse("0000,100x30,0,0{55x30,0,0,0,44x30,56,0,1}")
            .unwrap()
            .serialize();
        assert_eq!(said(&out), [format!("select-layout -t @0 '{layout}'")]);
    }

    #[test]
    fn a_line_that_arrives_in_pieces_is_one_line() {
        let mut engine = attached();
        refreshed(&mut engine, TWO_PANES);
        let mut out = Out::default();
        engine.act(Action::Watch { pane: 0 }, &mut out);
        answer(
            &mut engine,
            &["", "", "", "", "0,0,0,1,0,0,0,0,0,0,0,29,30\n", "x\n"],
        );

        let mut out = Out::default();
        engine.feed(b"%output %0 he", &mut out);
        engine.feed(b"llo\n%exit server exited\n", &mut out);
        assert_eq!(
            out.events,
            [Event::Output {
                pane: 0,
                data: b"hello".to_vec()
            }]
        );
        assert_eq!(engine.reason(), Some("server exited"));
    }

    #[test]
    fn a_kept_terminal_shows_its_pane_unasked_and_is_typed_at_without_naming_it() {
        let mut engine = Engine::following(100, 30, 2000);
        let mut out = Out::default();
        engine.feed(b"%begin 1 272 0\n%end 1 272 0\n", &mut out);
        let out = answer(
            &mut engine,
            &[
                "",
                "",
                "$4\n",
                "$4\t1\t1\tt-abc\n",
                "$4\t@7\t0\t1\t0\ta880,100x30,0,0,9\tbash\n",
                "%9\t@7\t1\t500\t/dev/pts/2\tbash\n",
            ],
        );
        // Asked for whole at once, with no one having asked.
        assert!(said(&out)
            .iter()
            .any(|line| line.starts_with("capture-pane") && line.ends_with("%9")));
        let out = answer(&mut engine, &["0,0,0,1,0,0,0,0,0,0,0,29,30\n", "prompt\n"]);
        assert!(matches!(&out.events[..], [Event::Output { pane: 9, .. }]));

        let mut out = Out::default();
        engine.act(
            Action::Type {
                bytes: b"ls\r".to_vec(),
            },
            &mut out,
        );
        assert_eq!(said(&out), ["send-keys -t %9 -H 6c 73 0d"]);
        let mut out = Out::default();
        engine.act(Action::End, &mut out);
        assert_eq!(said(&out), ["kill-session"]);

        assert!(!engine.exited());
        engine.feed(b"%exit\n", &mut Out::default());
        assert!(engine.exited());
    }

    #[test]
    fn starting_names_the_session_or_makes_one() {
        let kept = Start::Kept {
            name: "t-ab;c'1".into(),
            cwd: Some("/srv/my app".into()),
        };
        let line = kept.arguments(120, 40);
        assert!(
            line.starts_with("-L tide -u -C start-server \\; if-shell -F"),
            "{line}"
        );
        assert!(
            line.ends_with("new-session -A -s 't-abc1' -x 120 -y 40 -c '/srv/my app'"),
            "{line}"
        );
        assert_eq!(
            Start::Attach(3).arguments(80, 24),
            "-u -C attach-session -t '$3'"
        );
        assert_eq!(
            Start::New("it's".into()).arguments(120, 40),
            r"-u -C new-session -x 120 -y 40 -s 'it'\''s'"
        );
        assert_eq!(
            Start::New(String::new()).arguments(120, 40),
            "-u -C new-session -x 120 -y 40"
        );
    }
}
