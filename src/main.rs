use anyhow::{Context, Result};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};
use std::{
    collections::{HashMap, HashSet},
    io,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use sysinfo::{Pid, Signal, System};

// ─── Modes ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    Orphans,
    All,
    Tracked,
}

impl ViewMode {
    fn label(&self) -> &'static str {
        match self {
            ViewMode::Orphans => "ORPHANS",
            ViewMode::All => "ALL PROCS",
            ViewMode::Tracked => "TRACKED",
        }
    }

    fn next(&self) -> ViewMode {
        match self {
            ViewMode::Orphans => ViewMode::All,
            ViewMode::All => ViewMode::Tracked,
            ViewMode::Tracked => ViewMode::Orphans,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum AppMode {
    Normal,
    LaunchInput,
}

// ─── Data types ────────────────────────────────────────────────────────

#[derive(Clone)]
struct ProcessInfo {
    pid: u32,
    name: String,
    memory_mb: f64,
    ports: Vec<u16>,
    path: String,
    is_tracked: bool,
    is_orphan: bool,
}

struct TrackedEntry {
    pid: u32,
    label: String,
    launched_at: Instant,
    children: HashSet<u32>,
}

// ─── App ──────────────────────────────────────────────────────────────

struct App {
    state: ListState,
    items: Vec<ProcessInfo>,
    system: System,
    last_refresh: Instant,
    port_map: HashMap<u32, Vec<u16>>,
    view_mode: ViewMode,
    app_mode: AppMode,
    launch_input: String,
    tracked: Vec<TrackedEntry>,
    status_msg: Option<(String, Instant)>,
}

// ─── helpers ──────────────────────────────────────────────────────────

fn all_descendants(system: &System, root_pid: u32) -> HashSet<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (pid, process) in system.processes() {
        if let Some(parent) = process.parent() {
            children.entry(parent.as_u32()).or_default().push(pid.as_u32());
        }
    }

    let mut set = HashSet::new();
    let mut stack = vec![root_pid];
    while let Some(pid) = stack.pop() {
        if set.insert(pid) {
            if let Some(kids) = children.get(&pid) {
                stack.extend(kids);
            }
        }
    }
    set.remove(&root_pid);
    set
}

// ─── impl App ─────────────────────────────────────────────────────────

impl App {
    fn new() -> App {
        let mut system = System::new_all();
        system.refresh_all();
        App {
            state: ListState::default(),
            items: Vec::new(),
            system,
            last_refresh: Instant::now(),
            port_map: HashMap::new(),
            view_mode: ViewMode::Orphans,
            app_mode: AppMode::Normal,
            launch_input: String::new(),
            tracked: Vec::new(),
            status_msg: None,
        }
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_msg = Some((msg.into(), Instant::now()));
    }

    fn refresh_data(&mut self) {
        self.system.refresh_all();
        self.refresh_port_map();

        // Update tracked process trees
        for entry in &mut self.tracked {
            entry.children = all_descendants(&self.system, entry.pid);
        }

        // Remove dead tracked roots
        let running_pids: HashSet<u32> =
            self.system.processes().keys().map(|p| p.as_u32()).collect();
        self.tracked.retain(|e| running_pids.contains(&e.pid));

        // Build the tracked PIDs set (root + all children)
        let mut tracked_set: HashSet<u32> = HashSet::new();
        for entry in &self.tracked {
            tracked_set.insert(entry.pid);
            tracked_set.extend(&entry.children);
        }

        // Suspicious orphan names
        let suspicious_names = [
            "node", "python", "ruby", "java", "chrome", "chromium", "brave",
            "esbuild", "vite", "next-server", "webpack", "docker", "rustc",
        ];

        let mut all_procs: Vec<ProcessInfo> = Vec::new();

        for (pid, process) in self.system.processes() {
            let pid_u32 = pid.as_u32();
            let ppid = process.parent().map(|p| p.as_u32()).unwrap_or(0);
            let name = process.name().to_string_lossy().to_lowercase();
            let is_orphan =
                ppid == 1 && suspicious_names.iter().any(|&s| name.contains(s));

            let is_tracked = tracked_set.contains(&pid_u32);

            if !is_orphan && !is_tracked {
                continue;
            }

            all_procs.push(ProcessInfo {
                pid: pid_u32,
                name: process.name().to_string_lossy().into_owned(),
                memory_mb: process.memory() as f64 / 1024.0 / 1024.0,
                ports: self.port_map.get(&pid_u32).cloned().unwrap_or_default(),
                path: process
                    .exe()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                is_tracked,
                is_orphan,
            });
        }

        // Apply view mode filter
        let filtered = match self.view_mode {
            ViewMode::Orphans => {
                all_procs.into_iter().filter(|p| p.is_orphan && !p.is_tracked).collect()
            }
            ViewMode::Tracked => {
                all_procs.into_iter().filter(|p| p.is_tracked).collect()
            }
            ViewMode::All => all_procs,
        };

        self.items = filtered;
        self.items.sort_by(|a, b| {
            b.memory_mb
                .partial_cmp(&a.memory_mb)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Fix selection bounds
        if self.items.is_empty() {
            self.state.select(None);
        } else if let Some(i) = self.state.selected() {
            if i >= self.items.len() {
                self.state.select(Some(self.items.len() - 1));
            }
        } else {
            self.state.select(Some(0));
        }
    }

    fn refresh_port_map(&mut self) {
        self.port_map.clear();
        let Ok(all_procs) = procfs::process::all_processes() else {
            return;
        };
        for p_res in all_procs {
            let Ok(p) = p_res else { continue };
            let Ok(fds) = p.fd() else { continue };
            for fd_res in fds {
                let Ok(fd) = fd_res else { continue };
                let procfs::process::FDTarget::Socket(inode) = fd.target else { continue };
                let Ok(tcp) = procfs::net::tcp() else { continue };
                for entry in tcp {
                    if entry.inode == inode {
                        self.port_map
                            .entry(p.pid as u32)
                            .or_default()
                            .push(entry.local_address.port());
                    }
                }
            }
        }
    }

    fn launch_app(&mut self, command_text: &str) {
        let command_text = command_text.trim();
        if command_text.is_empty() {
            self.set_status("no command entered");
            return;
        }

        match Command::new("sh")
            .args(["-c", command_text])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => {
                let pid = child.id();
                self.tracked.push(TrackedEntry {
                    pid,
                    label: command_text.to_string(),
                    launched_at: Instant::now(),
                    children: HashSet::new(),
                });
                self.set_status(format!("launched [{}] pid={}", command_text, pid));
                self.view_mode = ViewMode::Tracked;
                self.refresh_data();
            }
            Err(e) => {
                self.set_status(format!("launch failed: {}", e));
            }
        }
    }

    fn kill_selected(&mut self) {
        let Some(index) = self.state.selected() else { return };
        let Some(info) = self.items.get(index) else { return };

        let pid = info.pid;

        // Kill the process
        if let Some(process) = self.system.process(Pid::from_u32(pid)) {
            let _ = process.kill_with(Signal::Term);
        }

        // If it's a tracked root, kill its children too
        for entry in &self.tracked {
            if entry.pid == pid {
                for &child_pid in &entry.children {
                    if let Some(child) = self.system.process(Pid::from_u32(child_pid)) {
                        let _ = child.kill_with(Signal::Term);
                    }
                }
                break;
            }
        }

        self.set_status(format!("terminated pid={}", pid));
        self.refresh_data();
    }

    fn next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) if i >= self.items.len() - 1 => 0,
            Some(i) => i + 1,
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) if i == 0 => self.items.len() - 1,
            Some(i) => i - 1,
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn remove_dead_tracked(&mut self) {
        let running: HashSet<u32> =
            self.system.processes().keys().map(|p| p.as_u32()).collect();
        // Collect labels BEFORE any mutable borrow
        let dead_labels: Vec<String> = self
            .tracked
            .iter()
            .filter(|e| !running.contains(&e.pid))
            .map(|e| e.label.clone())
            .collect();
        for label in &dead_labels {
            self.set_status(format!("tracked process ended: {}", label));
        }
        self.tracked.retain(|e| running.contains(&e.pid));
        self.refresh_data();
    }
}

// ─── TUI helpers ──────────────────────────────────────────────────────

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// ─── UI ───────────────────────────────────────────────────────────────

fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(f.area());

    // ─── header ───────────────────────────────────────────────────
    let mode_tag = format!(
        " Orphan Process Manager — [{}]  Tab:mode  a:launch  j/k:navigate  x:kill  q:quit ",
        app.view_mode.label()
    );
    let header = Paragraph::new(mode_tag)
        .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, chunks[0]);

    // ─── process list ──────────────────────────────────────────────
    let tracked_root_pids: HashSet<u32> = app.tracked.iter().map(|e| e.pid).collect();

    let list_items: Vec<ListItem> = app
        .items
        .iter()
        .map(|p| {
            let ports_str = if p.ports.is_empty() {
                String::new()
            } else {
                format!(" [:{:?}]", p.ports)
            };

            let line = format!(
                " {:^7} | {:16} | {:>8.2} MB{}",
                p.pid, p.name, p.memory_mb, ports_str
            );

            let style = if p.is_tracked && tracked_root_pids.contains(&p.pid) {
                // Root tracked process — bold green
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else if p.is_tracked {
                // Child of tracked process — green
                Style::default().fg(Color::Green)
            } else if p.is_orphan {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let title = match app.view_mode {
        ViewMode::Orphans => " Orphaned Background Processes ",
        ViewMode::All => " All Processes ",
        ViewMode::Tracked => " Tracked Processes ",
    };

    let list = List::new(list_items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(list, chunks[1], &mut app.state);

    // ─── footer ───────────────────────────────────────────────────
    let footer_text = if let Some(index) = app.state.selected() {
        if let Some(item) = app.items.get(index) {
            let tags = if item.is_tracked { " [TRACKED]" } else { "" };
            format!(" {}{}  |  [X] Terminate  [R] Refresh  [Q] Exit", item.path, tags)
        } else {
            " [R] Refresh  [Q] Exit".to_string()
        }
    } else if let Some((msg, _)) = &app.status_msg {
        format!(" {}", msg)
    } else {
        " [R] Refresh  [Q] Exit".to_string()
    };

    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, chunks[2]);

    // ─── launch input popup ───────────────────────────────────────
    if let AppMode::LaunchInput = app.app_mode {
        let area = centered_rect(60, 20, f.area());
        f.render_widget(Clear, area);

        let input_display = if app.launch_input.is_empty() {
            " type a command…".to_string()
        } else {
            format!(" {}", app.launch_input)
        };

        let popup = Paragraph::new(input_display)
            .block(
                Block::default()
                    .title(" Launch App ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .style(Style::default().fg(Color::White));
        f.render_widget(popup, area);
    }
}

// ─── main loop ────────────────────────────────────────────────────────

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> Result<()> {
    app.refresh_data();

    loop {
        terminal
            .draw(|f| ui(f, &mut app))
            .map_err(|e| anyhow::anyhow!("render error: {e}"))?;

        // Auto-refresh every 5s
        if app.last_refresh.elapsed() > Duration::from_secs(5) {
            app.remove_dead_tracked();
            app.refresh_data();
            app.last_refresh = Instant::now();
        }

        // Clear status messages after 8s
        if let Some((_, at)) = &app.status_msg {
            if at.elapsed() > Duration::from_secs(8) {
                app.status_msg = None;
            }
        }

        if !event::poll(Duration::from_millis(200)).context("event poll")? {
            continue;
        }

        let Event::Key(key) = event::read().context("event read")? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match app.app_mode {
            AppMode::LaunchInput => match key.code {
                KeyCode::Esc => {
                    app.app_mode = AppMode::Normal;
                    app.launch_input.clear();
                }
                KeyCode::Enter => {
                    let cmd = app.launch_input.clone();
                    app.app_mode = AppMode::Normal;
                    app.launch_input.clear();
                    app.launch_app(&cmd);
                }
                KeyCode::Backspace => {
                    app.launch_input.pop();
                }
                KeyCode::Char(c) => {
                    app.launch_input.push(c);
                }
                _ => {}
            },

            AppMode::Normal => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('a') => {
                    app.app_mode = AppMode::LaunchInput;
                    app.launch_input.clear();
                }
                KeyCode::Tab => {
                    app.view_mode = app.view_mode.next();
                    app.refresh_data();
                }
                KeyCode::Down | KeyCode::Char('j') => app.next(),
                KeyCode::Up | KeyCode::Char('k') => app.previous(),
                KeyCode::Char('r') => {
                    app.remove_dead_tracked();
                    app.refresh_data();
                    app.set_status("refreshed");
                }
                KeyCode::Enter | KeyCode::Char('x') => app.kill_selected(),
                _ => {}
            },
        }
    }

    Ok(())
}

// ─── entry point ──────────────────────────────────────────────────────

fn main() -> Result<()> {
    enable_raw_mode().context("raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture).context("terminal setup")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("terminal init")?;

    let app = App::new();
    let res = run_app(&mut terminal, app);

    disable_raw_mode().context("raw mode cleanup")?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .context("terminal cleanup")?;
    terminal.show_cursor().context("cursor restore")?;

    if let Err(ref err) = res {
        eprintln!("error: {err:#}");
    }

    Ok(())
}
