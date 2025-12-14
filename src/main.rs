use std::collections::{HashMap, HashSet};
use std::fs;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    DefaultTerminal, Frame,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct UsbDevice {
    bus: String,
    device: String,
    vendor_id: String,
    product_id: String,
    name: String,
    is_dfu: bool,
    dev_path: String,       // /dev/bus/usb/BUS/DEVICE or tty path
    tty_path: Option<String>, // /dev/ttyUSB0, /dev/ttyACM0, etc.
}

impl UsbDevice {
    /// Unique key for this specific device (bus + device number)
    fn key(&self) -> String {
        format!("{}:{}", self.bus, self.device)
    }

    fn id(&self) -> String {
        format!("{}:{}", self.vendor_id, self.product_id)
    }

    /// Display path - prefer tty over bus path
    fn display_path(&self) -> &str {
        self.tty_path.as_deref().unwrap_or(&self.dev_path)
    }
}

/// Build a map of (bus, devnum) -> tty device path by scanning /dev/serial/by-path
/// This is fast because it just reads symlinks
fn get_tty_map() -> HashMap<(u32, u32), String> {
    let mut map = HashMap::new();

    // Method 1: Check /dev/serial/by-id (fastest, has nice names)
    if let Ok(entries) = fs::read_dir("/dev/serial/by-id") {
        for entry in entries.flatten() {
            if let Ok(target) = fs::read_link(entry.path()) {
                let target_str = target.to_string_lossy();
                // Extract ttyUSB0 or ttyACM0 from the target
                if let Some(tty_name) = target_str.strip_prefix("../../") {
                    if tty_name.starts_with("ttyUSB") || tty_name.starts_with("ttyACM") {
                        // Now find which bus/dev this corresponds to
                        if let Some((bus, dev)) = get_tty_bus_dev(tty_name) {
                            map.insert((bus, dev), format!("/dev/{}", tty_name));
                        }
                    }
                }
            }
        }
    }

    // Method 2: Direct scan of /dev/ttyUSB* and /dev/ttyACM*
    for prefix in &["ttyUSB", "ttyACM"] {
        for i in 0..16 {
            let tty_name = format!("{}{}", prefix, i);
            if let Some((bus, dev)) = get_tty_bus_dev(&tty_name) {
                map.entry((bus, dev)).or_insert_with(|| format!("/dev/{}", tty_name));
            }
        }
    }

    map
}

/// Get bus and device number for a tty device by reading sysfs
fn get_tty_bus_dev(tty_name: &str) -> Option<(u32, u32)> {
    // Read /sys/class/tty/ttyUSB0/device/../.. to find the USB device
    let device_path = format!("/sys/class/tty/{}/device", tty_name);
    
    // Follow symlinks to find the USB device directory
    let real_path = fs::canonicalize(&device_path).ok()?;
    
    // Walk up to find busnum/devnum
    let mut current = real_path.as_path();
    for _ in 0..5 {
        current = current.parent()?;
        let busnum_path = current.join("busnum");
        let devnum_path = current.join("devnum");
        
        if busnum_path.exists() && devnum_path.exists() {
            let bus: u32 = fs::read_to_string(&busnum_path).ok()?.trim().parse().ok()?;
            let dev: u32 = fs::read_to_string(&devnum_path).ok()?.trim().parse().ok()?;
            return Some((bus, dev));
        }
    }
    
    None
}

fn get_usb_devices() -> Vec<UsbDevice> {
    let output = Command::new("lsusb").output();
    let tty_map = get_tty_map();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout
                .lines()
                .filter_map(|line| parse_lsusb_line(line, &tty_map))
                .collect()
        }
        Err(_) => vec![],
    }
}

fn parse_lsusb_line(line: &str, tty_map: &HashMap<(u32, u32), String>) -> Option<UsbDevice> {
    // Parse: Bus 001 Device 002: ID 1234:5678 Device Name
    let parts: Vec<&str> = line.splitn(2, ": ID ").collect();
    if parts.len() != 2 {
        return None;
    }

    let prefix = parts[0];
    let suffix = parts[1];

    // Parse bus and device from prefix
    let prefix_parts: Vec<&str> = prefix.split_whitespace().collect();
    if prefix_parts.len() < 4 {
        return None;
    }

    let bus = prefix_parts[1].to_string();
    let device = prefix_parts[3].to_string();

    // Parse ID and name from suffix
    let id_and_name: Vec<&str> = suffix.splitn(2, ' ').collect();
    let id = id_and_name[0];
    let name = if id_and_name.len() > 1 {
        id_and_name[1].to_string()
    } else {
        "Unknown".to_string()
    };

    let id_parts: Vec<&str> = id.split(':').collect();
    if id_parts.len() != 2 {
        return None;
    }

    let vendor_id = id_parts[0].to_string();
    let product_id = id_parts[1].to_string();

    let name_lower = name.to_lowercase();
    let is_dfu = name_lower.contains("dfu")
        || name_lower.contains("download")
        || name_lower.contains("boot");

    // Build /dev/bus/usb path
    let dev_path = format!("/dev/bus/usb/{}/{}", bus, device);

    // Look up tty path
    let bus_num: u32 = bus.parse().unwrap_or(0);
    let dev_num: u32 = device.parse().unwrap_or(0);
    let tty_path = tty_map.get(&(bus_num, dev_num)).cloned();

    Some(UsbDevice {
        bus,
        device,
        vendor_id,
        product_id,
        name,
        is_dfu,
        dev_path,
        tty_path,
    })
}

// Stats tracking
struct Stats {
    start_time: Instant,
    refresh_count: u64,
    devices_ever_seen: HashSet<String>,
    dfu_devices_ever_seen: HashSet<String>,
    last_refresh_duration: Duration,
    peak_devices: usize,
    connects: u64,
    disconnects: u64,
}

impl Stats {
    fn new() -> Self {
        Self {
            start_time: Instant::now(),
            refresh_count: 0,
            devices_ever_seen: HashSet::new(),
            dfu_devices_ever_seen: HashSet::new(),
            last_refresh_duration: Duration::ZERO,
            peak_devices: 0,
            connects: 0,
            disconnects: 0,
        }
    }

    fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }

    fn format_uptime(&self) -> String {
        let secs = self.uptime().as_secs();
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        let secs = secs % 60;
        if hours > 0 {
            format!("{:02}:{:02}:{:02}", hours, mins, secs)
        } else {
            format!("{:02}:{:02}", mins, secs)
        }
    }

    fn refresh_rate(&self) -> f64 {
        let elapsed = self.uptime().as_secs_f64();
        if elapsed > 0.0 {
            self.refresh_count as f64 / elapsed
        } else {
            0.0
        }
    }
}

struct App {
    devices: Vec<UsbDevice>,
    list_state: ListState,
    selected_key: Option<String>, // Track selection by device key, not index
    should_quit: bool,
    stats: Stats,
    device_receiver: Receiver<(Vec<UsbDevice>, Duration)>,
    refresh_trigger: Sender<()>,
}

impl App {
    fn new() -> Self {
        let (device_tx, device_rx) = mpsc::channel();
        let (trigger_tx, trigger_rx) = mpsc::channel::<()>();

        // Spawn background thread for USB polling
        thread::spawn(move || {
            loop {
                // Wait for trigger or timeout (5Hz = 200ms)
                let _ = trigger_rx.recv_timeout(Duration::from_millis(200));

                let start = Instant::now();
                let devices = get_usb_devices();
                let duration = start.elapsed();

                if device_tx.send((devices, duration)).is_err() {
                    break; // Main thread closed, exit
                }
            }
        });

        // Trigger initial refresh
        let _ = trigger_tx.send(());

        let mut app = Self {
            devices: vec![],
            list_state: ListState::default(),
            selected_key: None,
            should_quit: false,
            stats: Stats::new(),
            device_receiver: device_rx,
            refresh_trigger: trigger_tx,
        };

        // Wait for initial data
        if let Ok((devices, duration)) = app.device_receiver.recv_timeout(Duration::from_secs(1)) {
            app.update_devices(devices, duration);
        }

        app
    }

    fn update_devices(&mut self, new_devices: Vec<UsbDevice>, refresh_duration: Duration) {
        // Track connects/disconnects using unique keys
        let old_keys: HashSet<String> = self.devices.iter().map(|d| d.key()).collect();
        let new_keys: HashSet<String> = new_devices.iter().map(|d| d.key()).collect();

        if self.stats.refresh_count > 0 {
            self.stats.connects += new_keys.difference(&old_keys).count() as u64;
            self.stats.disconnects += old_keys.difference(&new_keys).count() as u64;
        }

        self.devices = new_devices;
        self.stats.refresh_count += 1;
        self.stats.last_refresh_duration = refresh_duration;

        // Update stats
        if self.devices.len() > self.stats.peak_devices {
            self.stats.peak_devices = self.devices.len();
        }
        for device in &self.devices {
            self.stats.devices_ever_seen.insert(device.id());
            if device.is_dfu {
                self.stats.dfu_devices_ever_seen.insert(device.id());
            }
        }

        // Restore selection by key
        if let Some(ref key) = self.selected_key {
            if let Some(idx) = self.devices.iter().position(|d| d.key() == *key) {
                self.list_state.select(Some(idx));
            } else {
                // Device gone, keep index if valid
                let current = self.list_state.selected().unwrap_or(0);
                let new_idx = current.min(self.devices.len().saturating_sub(1));
                if !self.devices.is_empty() {
                    self.list_state.select(Some(new_idx));
                    self.selected_key = Some(self.devices[new_idx].key());
                }
            }
        } else if !self.devices.is_empty() {
            self.list_state.select(Some(0));
            self.selected_key = Some(self.devices[0].key());
        }
    }

    fn try_receive_devices(&mut self) {
        // Non-blocking receive - only take the latest update
        let mut latest: Option<(Vec<UsbDevice>, Duration)> = None;
        while let Ok(update) = self.device_receiver.try_recv() {
            latest = Some(update);
        }
        if let Some((devices, duration)) = latest {
            self.update_devices(devices, duration);
        }
    }

    fn manual_refresh(&mut self) {
        let _ = self.refresh_trigger.send(());
    }

    fn selected_device(&self) -> Option<&UsbDevice> {
        self.list_state
            .selected()
            .and_then(|i| self.devices.get(i))
    }

    fn next(&mut self) {
        if self.devices.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.devices.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
        self.selected_key = Some(self.devices[i].key());
    }

    fn previous(&mut self) {
        if self.devices.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.devices.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
        self.selected_key = Some(self.devices[i].key());
    }

    fn dfu_count(&self) -> usize {
        self.devices.iter().filter(|d| d.is_dfu).count()
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    let result = run(terminal);
    ratatui::restore();
    result
}

fn run(mut terminal: DefaultTerminal) -> Result<()> {
    let mut app = App::new();

    loop {
        // Check for new device data (non-blocking)
        app.try_receive_devices();

        terminal.draw(|frame| ui(frame, &mut app))?;

        // Poll for events with short timeout for responsive UI
        if event::poll(Duration::from_millis(16))? {
            // ~60fps UI
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                        KeyCode::Char('r') => app.manual_refresh(),
                        KeyCode::Down | KeyCode::Char('j') => app.next(),
                        KeyCode::Up | KeyCode::Char('k') => app.previous(),
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

fn ui(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Main layout: header, content, footer
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(5),    // Content
            Constraint::Length(3), // Footer
        ])
        .split(area);

    // Header
    render_header(frame, main_layout[0], app);

    // Content: device list on left, details on right
    let content_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(55), // Device list
            Constraint::Percentage(45), // Details panel
        ])
        .split(main_layout[1]);

    render_device_list(frame, content_layout[0], app);
    render_details(frame, content_layout[1], app);

    // Footer
    render_footer(frame, main_layout[2], app);
}

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let dfu_count = app.dfu_count();
    let mut spans = vec![
        Span::styled("USB Devices ", Style::default().fg(Color::Cyan).bold()),
        Span::styled(
            format!("({})", app.devices.len()),
            Style::default().fg(Color::DarkGray),
        ),
    ];

    if dfu_count > 0 {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            format!(" {} DFU ", dfu_count),
            Style::default()
                .fg(Color::White)
                .bg(Color::Magenta)
                .bold(),
        ));
    }

    // Add uptime on the right
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        format!("uptime {}", app.stats.format_uptime()),
        Style::default().fg(Color::DarkGray),
    ));

    let header = Paragraph::new(Line::from(spans))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        )
        .style(Style::default());

    frame.render_widget(header, area);
}

fn render_device_list(frame: &mut Frame, area: Rect, app: &mut App) {
    let items: Vec<ListItem> = app
        .devices
        .iter()
        .map(|device| {
            let name_style = if device.is_dfu {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default()
            };

            let path = device.display_path();
            let path_style = if device.tty_path.is_some() {
                Style::default().fg(Color::Green) // TTY paths in green
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let content = Line::from(vec![
                Span::styled(&device.name, name_style),
                Span::raw(" "),
                Span::styled(path, path_style),
            ]);

            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Devices ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(list, area, &mut app.list_state);
}

fn render_details(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title(" Details ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split details area: device info on top, stats on bottom
    let detail_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),     // Device details
            Constraint::Length(10), // Stats
        ])
        .split(inner);

    // Device details
    if let Some(device) = app.selected_device() {
        let mut lines = vec![
            Line::from(vec![
                Span::styled("Name     ", Style::default().fg(Color::DarkGray)),
                Span::styled(&device.name, Style::default().bold()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("ID       ", Style::default().fg(Color::DarkGray)),
                Span::styled(device.id(), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("Bus      ", Style::default().fg(Color::DarkGray)),
                Span::raw(&device.bus),
            ]),
            Line::from(vec![
                Span::styled("Device   ", Style::default().fg(Color::DarkGray)),
                Span::raw(&device.device),
            ]),
            Line::from(vec![
                Span::styled("Vendor   ", Style::default().fg(Color::DarkGray)),
                Span::raw(&device.vendor_id),
            ]),
            Line::from(vec![
                Span::styled("Product  ", Style::default().fg(Color::DarkGray)),
                Span::raw(&device.product_id),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Path     ", Style::default().fg(Color::DarkGray)),
                Span::styled(&device.dev_path, Style::default().fg(Color::Green)),
            ]),
        ];

        // Show tty if present
        if let Some(ref tty) = device.tty_path {
            lines.push(Line::from(vec![
                Span::styled("TTY      ", Style::default().fg(Color::DarkGray)),
                Span::styled(tty, Style::default().fg(Color::Green).bold()),
            ]));
        }

        if device.is_dfu {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "⚡ DFU Mode",
                Style::default().fg(Color::Yellow).bold(),
            )));
        }

        let details = Paragraph::new(lines).wrap(Wrap { trim: true });
        frame.render_widget(details, detail_layout[0]);
    } else {
        let no_device = Paragraph::new("No device selected")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(no_device, detail_layout[0]);
    }

    // Stats section
    render_stats(frame, detail_layout[1], app);
}

fn render_stats(frame: &mut Frame, area: Rect, app: &App) {
    let stats = &app.stats;

    let refresh_ms = stats.last_refresh_duration.as_micros() as f64 / 1000.0;
    let rate = stats.refresh_rate();

    let lines = vec![
        Line::from(Span::styled(
            "─── Stats ───",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(vec![
            Span::styled("Refreshes    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", stats.refresh_count),
                Style::default().fg(Color::Green),
            ),
            Span::styled(
                format!(" ({:.1}/s)", rate),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("Latency      ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:.2}ms", refresh_ms),
                if refresh_ms < 10.0 {
                    Style::default().fg(Color::Green)
                } else if refresh_ms < 50.0 {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::Red)
                },
            ),
        ]),
        Line::from(vec![
            Span::styled("Peak         ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{} devices", stats.peak_devices)),
        ]),
        Line::from(vec![
            Span::styled("Ever seen    ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{} unique", stats.devices_ever_seen.len())),
        ]),
        Line::from(vec![
            Span::styled("DFU seen     ", Style::default().fg(Color::DarkGray)),
            if stats.dfu_devices_ever_seen.is_empty() {
                Span::styled("none", Style::default().fg(Color::DarkGray))
            } else {
                Span::styled(
                    format!("{}", stats.dfu_devices_ever_seen.len()),
                    Style::default().fg(Color::Magenta).bold(),
                )
            },
        ]),
        Line::from(vec![
            Span::styled("Connects     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("+{}", stats.connects),
                Style::default().fg(Color::Green),
            ),
            Span::raw(" / "),
            Span::styled(
                format!("-{}", stats.disconnects),
                Style::default().fg(Color::Red),
            ),
        ]),
    ];

    let stats_widget = Paragraph::new(lines);
    frame.render_widget(stats_widget, area);
}

fn render_footer(frame: &mut Frame, area: Rect, app: &App) {
    let refresh_indicator = if app.stats.refresh_count % 2 == 0 {
        "●"
    } else {
        "○"
    };

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(refresh_indicator, Style::default().fg(Color::Green)),
        Span::raw(" "),
        Span::styled("↑/↓", Style::default().fg(Color::Cyan)),
        Span::raw(" navigate  "),
        Span::styled("r", Style::default().fg(Color::Cyan)),
        Span::raw(" refresh  "),
        Span::styled("q", Style::default().fg(Color::Cyan)),
        Span::raw(" quit"),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    )
    .style(Style::default().fg(Color::DarkGray));

    frame.render_widget(footer, area);
}
