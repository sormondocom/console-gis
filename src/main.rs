use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    style::{Color, Modifier, Style},
    Terminal,
};

use console_gis::{
    data::{Marker, MarkerStore},
    render::{detect_capability, canvas::TerminalCapability},
    tui::{
        app::{App, MarkerInput, MarkerInputStep, SavedState, View},
        views::{
            splash::SplashWidget,
            menu::{MenuWidget, MENU_ITEMS},
            globe::GlobeView,
            map::{MapView, pan_lat_step, pan_lon_step},
            markers::MarkerListView,
            zoom_explorer::ZoomExplorerView,
            diagnostics::DiagnosticsView,
        },
    },
};

fn main() -> anyhow::Result<()> {
    let capability = detect_capability();

    let data_dir = dirs_next::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("console-gis");
    std::fs::create_dir_all(&data_dir)?;

    let marker_path = data_dir.join("markers");
    let state_path  = data_dir.join("state.json");

    let markers = MarkerStore::open(&marker_path)?;
    let saved   = SavedState::load(&state_path);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend  = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;

    let result = run_app(&mut term, capability, markers, state_path, &saved);

    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    term.show_cursor()?;

    if let Err(e) = result { eprintln!("Error: {e}"); }
    Ok(())
}

// ── Main event loop ───────────────────────────────────────────────────────────

fn run_app(
    term:          &mut Terminal<CrosstermBackend<io::Stdout>>,
    capability:    TerminalCapability,
    markers_store: MarkerStore,
    state_path:    std::path::PathBuf,
    saved:         &SavedState,
) -> io::Result<()> {
    let mut app = App::new(capability, markers_store, state_path, saved);

    let restore_warnings = app.restore_layers(&saved.layer_paths.clone());
    if !restore_warnings.is_empty() {
        app.import_error = Some(restore_warnings.join("\n"));
    }

    let mut menu_sel: usize = 0;
    let splash_start = Instant::now();
    let tick_rate    = Duration::from_millis(50);
    let mut last_tick = Instant::now();

    loop {
        let size = term.size()?;
        let (cols, rows) = (size.width, size.height);
        let all_markers: Vec<_> = app.markers.all().unwrap_or_default();

        // ── Draw ──────────────────────────────────────────────────────────────
        term.draw(|f| {
            let area = f.area();

            match app.view {
                View::Splash => {
                    f.render_widget(
                        SplashWidget {
                            rotation: app.globe.rot_y,
                            supports_true_colour: capability.supports_true_colour(),
                        },
                        area,
                    );
                }
                View::Menu => {
                    f.render_widget(
                        MenuWidget {
                            items:      MENU_ITEMS,
                            selected:   menu_sel,
                            true_color: capability.supports_true_colour(),
                        },
                        area,
                    );
                }
                View::Globe => {
                    f.render_widget(
                        GlobeView {
                            params:     &app.globe,
                            capability,
                            world:      &app.world,
                            markers:    &all_markers,
                            layers:     &app.geo_layers,
                            animating:  app.animating,
                            cursor_lat: app.globe_cursor.lat,
                            cursor_lon: app.globe_cursor.lon,
                            placing:    app.placing_marker,
                        },
                        area,
                    );
                }
                View::Map => {
                    f.render_widget(
                        MapView {
                            center_lat:  app.map_centre.lat,
                            center_lon:  app.map_centre.lon,
                            zoom:        app.zoom,
                            capability,
                            world:       &app.world,
                            markers:     &all_markers,
                            layers:      &app.geo_layers,
                            resolution:  &app.resolution,
                            placing:     app.placing_marker,
                        },
                        area,
                    );
                }
                View::MarkerList => {
                    let sel = app.marker_list_sel.min(all_markers.len().saturating_sub(1));
                    f.render_widget(
                        MarkerListView {
                            markers:    &all_markers,
                            selected:   sel,
                            capability,
                        },
                        area,
                    );
                }
                View::ZoomExplorer => {
                    f.render_widget(
                        ZoomExplorerView { zoom: app.zoom, cols, rows, capability },
                        area,
                    );
                }
                View::Diagnostics => {
                    f.render_widget(
                        DiagnosticsView {
                            capability,
                            cols,
                            rows,
                            char_aspect: app.resolution.char_aspect,
                        },
                        area,
                    );
                }
            }

            // ── Marker input overlay ──────────────────────────────────────────
            if let Some(ref mi) = app.marker_input {
                let buf = f.buffer_mut();
                let oy  = rows.saturating_sub(3);
                let tc  = capability.supports_true_colour();
                let bg  = if tc { Color::Rgb(5, 25, 45)    } else { Color::DarkGray };
                let fg  = if tc { Color::Rgb(30, 200, 240)  } else { Color::Cyan };
                let wfg = Color::White;

                let (title, input_text) = match mi.step {
                    MarkerInputStep::Symbol => (
                        " Marker symbol  (any character · Tab=● · Esc=cancel) ",
                        format!(" > {}█", mi.symbol_buf),
                    ),
                    MarkerInputStep::Label => (
                        " Marker label   (free text · Tab=\"Marker\" · Enter=next · Esc=cancel) ",
                        format!(" > {}█", mi.label_buf),
                    ),
                    MarkerInputStep::Blink => (
                        " Blink marker?  (Y=yes · N/Enter=no · Esc=cancel) ",
                        format!(" > blink: {}  [Y/N]", if mi.blink { "ON " } else { "off" }),
                    ),
                };

                let title_style = Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD);
                let input_style = Style::default().fg(wfg).bg(bg);

                let edit_label = if mi.edit_id.is_some() { "Edit" } else { "New" };
                let header = format!(
                    " {} marker at {:.3}°{} {:.3}°{} ",
                    edit_label,
                    mi.lat.abs(), if mi.lat >= 0.0 { 'N' } else { 'S' },
                    mi.lon.abs(), if mi.lon >= 0.0 { 'E' } else { 'W' },
                );
                let header_style = Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD);

                let rows_data: &[(&str, Style, &str)] = &[
                    (&header,     header_style, ""),
                    (title,       title_style,  ""),
                    (&input_text, input_style,  ""),
                ];
                for (row_off, (text, style, _)) in rows_data.iter().enumerate() {
                    let r = oy + row_off as u16;
                    for c in 0..cols { buf.get_mut(c, r).set_char(' ').set_style(*style); }
                    for (ci, ch) in text.chars().enumerate() {
                        let c = ci as u16;
                        if c >= cols { break; }
                        buf.get_mut(c, r).set_char(ch).set_style(*style);
                    }
                }
            }

            // ── GeoJSON import overlay ────────────────────────────────────────
            if app.importing {
                let buf = f.buffer_mut();
                let oy  = rows.saturating_sub(3);
                let tc  = capability.supports_true_colour();
                let bg  = if tc { Color::Rgb(10, 20, 40)  } else { Color::DarkGray };
                let fg  = if tc { Color::Rgb(30, 200, 240) } else { Color::Cyan };
                let ef  = if tc { Color::Rgb(220, 80, 80)  } else { Color::Red  };

                let title      = " Import GeoJSON — enter file path (Tab=clear  Esc=cancel  Enter=load) ";
                let input_line = format!(" > {}█", app.import_buf);
                let err_line   = match &app.import_error {
                    Some(e) => format!(" ⚠  {e}"),
                    None    => String::new(),
                };

                let title_style = Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD);
                let input_style = Style::default().fg(Color::White).bg(bg);
                let err_style   = Style::default().fg(ef).bg(bg);

                let rows_data: &[(&str, Style)] = &[
                    (title,        title_style),
                    (&input_line,  input_style),
                    (&err_line,    err_style),
                ];
                for (row_off, (text, style)) in rows_data.iter().enumerate() {
                    let r = oy + row_off as u16;
                    for c in 0..cols { buf.get_mut(c, r).set_char(' ').set_style(*style); }
                    for (ci, ch) in text.chars().enumerate() {
                        let c = ci as u16;
                        if c >= cols { break; }
                        buf.get_mut(c, r).set_char(ch).set_style(*style);
                    }
                }
            }

            // ── Bookmark name overlay ─────────────────────────────────────────
            if app.bookmarking {
                let buf = f.buffer_mut();
                let oy  = rows.saturating_sub(2);
                let tc  = capability.supports_true_colour();
                let bg  = if tc { Color::Rgb(5, 35, 15)    } else { Color::DarkGray };
                let fg  = if tc { Color::Rgb(80, 220, 100)  } else { Color::Green  };

                let view_tag = match app.view {
                    View::Globe => format!(
                        "globe @ {:.2}°{} {:.2}°{} z{:.1}",
                        app.globe_cursor.lat.abs(),
                        if app.globe_cursor.lat >= 0.0 { 'N' } else { 'S' },
                        app.globe_cursor.lon.abs(),
                        if app.globe_cursor.lon >= 0.0 { 'E' } else { 'W' },
                        app.globe.zoom,
                    ),
                    _ => format!(
                        "map @ {:.2}°{} {:.2}°{} z{}",
                        app.map_centre.lat.abs(),
                        if app.map_centre.lat >= 0.0 { 'N' } else { 'S' },
                        app.map_centre.lon.abs(),
                        if app.map_centre.lon >= 0.0 { 'E' } else { 'W' },
                        app.zoom,
                    ),
                };
                let title = format!(
                    " Bookmark {} — enter name (Tab=use coords · Enter=save · Esc=cancel) ",
                    view_tag,
                );
                let input_line = format!(" > {}█", app.bookmark_buf);

                let title_style = Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD);
                let input_style = Style::default().fg(Color::White).bg(bg);

                for (row_off, (text, style)) in [
                    (title.as_str(),   title_style),
                    (&input_line[..],  input_style),
                ].iter().enumerate() {
                    let r = oy + row_off as u16;
                    for c in 0..cols { buf.get_mut(c, r).set_char(' ').set_style(*style); }
                    for (ci, ch) in text.chars().enumerate() {
                        let c = ci as u16;
                        if c >= cols { break; }
                        buf.get_mut(c, r).set_char(ch).set_style(*style);
                    }
                }
            }

            // ── Clear-all confirmation overlay ────────────────────────────────
            if app.clearing_markers {
                let buf = f.buffer_mut();
                let oy  = rows.saturating_sub(2);
                let tc  = capability.supports_true_colour();
                let bg  = if tc { Color::Rgb(50, 5, 5)     } else { Color::DarkGray };
                let fg  = if tc { Color::Rgb(255, 80, 60)   } else { Color::Red };
                let kfg = if tc { Color::Rgb(220, 220, 220) } else { Color::White };

                let count = app.markers.count();
                let ms    = if count == 1 { "marker" } else { "markers" };
                let warn  = format!(
                    " ⚠  CLEAR ALL MARKERS — delete {} {} permanently? This cannot be undone.",
                    count, ms,
                );
                let keys  = "    [Y] yes, delete forever    [any other key] cancel".to_string();

                for (row_off, (text, style)) in [
                    (warn.as_str(), Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD)),
                    (keys.as_str(), Style::default().fg(kfg).bg(bg)),
                ].iter().enumerate() {
                    let r = oy + row_off as u16;
                    for c in 0..cols { buf.get_mut(c, r).set_char(' ').set_style(*style); }
                    for (ci, ch) in text.chars().enumerate() {
                        let c = ci as u16;
                        if c >= cols { break; }
                        buf.get_mut(c, r).set_char(ch).set_style(*style);
                    }
                }
            }

            // ── Single-marker delete confirmation (MarkerList view) ───────────
            if app.marker_del_confirm {
                let buf = f.buffer_mut();
                let oy  = rows.saturating_sub(2);
                let tc  = capability.supports_true_colour();
                let bg  = if tc { Color::Rgb(50, 5, 5)     } else { Color::DarkGray };
                let fg  = if tc { Color::Rgb(255, 80, 60)   } else { Color::Red };
                let kfg = if tc { Color::Rgb(220, 220, 220) } else { Color::White };
                let sel = app.marker_list_sel.min(all_markers.len().saturating_sub(1));
                let warn = if let Some(m) = all_markers.get(sel) {
                    format!(" ⚠  Delete marker #{} \"{}\" at {:.3}°, {:.3}°? Cannot be undone.",
                        m.id, m.label, m.lat, m.lon)
                } else {
                    " ⚠  Delete selected marker? Cannot be undone.".to_string()
                };
                let keys = "    [Y] yes · [any other key] cancel".to_string();

                for (row_off, (text, style)) in [
                    (warn.as_str(), Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD)),
                    (keys.as_str(), Style::default().fg(kfg).bg(bg)),
                ].iter().enumerate() {
                    let r = oy + row_off as u16;
                    for c in 0..cols { buf.get_mut(c, r).set_char(' ').set_style(*style); }
                    for (ci, ch) in text.chars().enumerate() {
                        let c = ci as u16;
                        if c >= cols { break; }
                        buf.get_mut(c, r).set_char(ch).set_style(*style);
                    }
                }
            }
        })?;

        // ── Event handling ────────────────────────────────────────────────────
        let timeout = tick_rate.checked_sub(last_tick.elapsed()).unwrap_or_default();
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }

                // Ctrl-C always quits
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('c')
                {
                    app.save_state();
                    return Ok(());
                }

                // ── Single-marker delete confirm ──────────────────────────────
                if app.marker_del_confirm {
                    if key.code == KeyCode::Char('y') || key.code == KeyCode::Char('Y') {
                        let all = app.markers.all().unwrap_or_default();
                        let sel = app.marker_list_sel.min(all.len().saturating_sub(1));
                        if let Some(m) = all.get(sel) {
                            let _ = app.markers.delete(m.id);
                        }
                        let new_count = app.markers.count();
                        if app.marker_list_sel >= new_count && new_count > 0 {
                            app.marker_list_sel = new_count - 1;
                        }
                    }
                    app.marker_del_confirm = false;
                    continue;
                }

                // ── Clear-all confirm ─────────────────────────────────────────
                if app.clearing_markers {
                    if key.code == KeyCode::Char('y') || key.code == KeyCode::Char('Y') {
                        let _ = app.markers.clear_all();
                        app.marker_list_sel = 0;
                    }
                    app.clearing_markers = false;
                    continue;
                }

                // ── Bookmark overlay ──────────────────────────────────────────
                if app.bookmarking {
                    match key.code {
                        KeyCode::Esc => {
                            app.bookmarking = false;
                            app.bookmark_buf.clear();
                        }
                        KeyCode::Tab => {
                            // Use coordinates as the bookmark name
                            let label = match app.view {
                                View::Globe => format!(
                                    "{:.2}°{} {:.2}°{} z{:.1}",
                                    app.globe_cursor.lat.abs(),
                                    if app.globe_cursor.lat >= 0.0 { 'N' } else { 'S' },
                                    app.globe_cursor.lon.abs(),
                                    if app.globe_cursor.lon >= 0.0 { 'E' } else { 'W' },
                                    app.globe.zoom,
                                ),
                                _ => format!(
                                    "{:.2}°{} {:.2}°{} z{}",
                                    app.map_centre.lat.abs(),
                                    if app.map_centre.lat >= 0.0 { 'N' } else { 'S' },
                                    app.map_centre.lon.abs(),
                                    if app.map_centre.lon >= 0.0 { 'E' } else { 'W' },
                                    app.zoom,
                                ),
                            };
                            app.bookmark_buf = label;
                        }
                        KeyCode::Backspace => { app.bookmark_buf.pop(); }
                        KeyCode::Enter => {
                            let label = if app.bookmark_buf.trim().is_empty() {
                                "Bookmark".to_string()
                            } else {
                                app.bookmark_buf.clone()
                            };
                            app.save_bookmark(&label);
                            app.bookmarking = false;
                            app.bookmark_buf.clear();
                        }
                        KeyCode::Char(c) => { app.bookmark_buf.push(c); }
                        _ => {}
                    }
                    continue;
                }

                // ── Marker input overlay ──────────────────────────────────────
                if app.marker_input.is_some() {
                    handle_marker_input(&mut app, key.code);
                    continue;
                }

                // ── Import overlay ────────────────────────────────────────────
                if app.importing {
                    match key.code {
                        KeyCode::Esc => { app.importing = false; app.import_error = None; }
                        KeyCode::Tab => { app.import_buf.clear(); app.import_error = None; }
                        KeyCode::Backspace => { app.import_buf.pop(); }
                        KeyCode::Enter => {
                            let path = app.import_buf.clone();
                            if app.load_geo_layer(&path) {
                                app.importing  = false;
                                app.import_buf = String::new();
                            }
                        }
                        KeyCode::Char(c) => { app.import_buf.push(c); }
                        _ => {}
                    }
                    continue;
                }

                // ── View-specific key handling ────────────────────────────────
                match app.view {
                    View::Splash => app.navigate(View::Menu),

                    View::Menu => match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') => {
                            app.save_state(); return Ok(());
                        }
                        KeyCode::Up   => { if menu_sel > 0 { menu_sel -= 1; } }
                        KeyCode::Down => {
                            if menu_sel + 1 < MENU_ITEMS.len() { menu_sel += 1; }
                        }
                        KeyCode::Enter => app.navigate(MENU_ITEMS[menu_sel].view),
                        KeyCode::Char(c) => {
                            if let Some(item) = MENU_ITEMS.iter().find(|i| i.key == c) {
                                app.navigate(item.view);
                            }
                        }
                        _ => {}
                    },

                    View::Globe => handle_globe_keys(&mut app, key.code),
                    View::Map   => handle_map_keys(&mut app, key.code),

                    View::MarkerList => {
                        let count = app.markers.count();
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('q') => app.navigate(View::Menu),
                            KeyCode::Up => {
                                if app.marker_list_sel > 0 { app.marker_list_sel -= 1; }
                            }
                            KeyCode::Down => {
                                if count > 0 && app.marker_list_sel + 1 < count {
                                    app.marker_list_sel += 1;
                                }
                            }
                            KeyCode::Char('e') | KeyCode::Char('E') => {
                                let all = app.markers.all().unwrap_or_default();
                                let sel = app.marker_list_sel.min(all.len().saturating_sub(1));
                                if let Some(m) = all.get(sel) {
                                    app.marker_input = Some(MarkerInput {
                                        lat:        m.lat,
                                        lon:        m.lon,
                                        symbol_buf: m.symbol.clone(),
                                        label_buf:  m.label.clone(),
                                        blink:      m.blink,
                                        step:       MarkerInputStep::Symbol,
                                        edit_id:    Some(m.id),
                                    });
                                }
                            }
                            KeyCode::Char('d') | KeyCode::Char('D') => {
                                if count > 0 { app.marker_del_confirm = true; }
                            }
                            KeyCode::Char('g') | KeyCode::Char('G') => {
                                let all = app.markers.all().unwrap_or_default();
                                let sel = app.marker_list_sel.min(all.len().saturating_sub(1));
                                if let Some(m) = all.get(sel) {
                                    let lat_r = m.lat.to_radians();
                                    let lon_r = m.lon.to_radians();
                                    app.globe.rot_y = lon_r;
                                    app.globe.rot_x = -lat_r;
                                    app.animating   = false;
                                    app.navigate(View::Globe);
                                }
                            }
                            KeyCode::Char('p') | KeyCode::Char('P') => {
                                let all = app.markers.all().unwrap_or_default();
                                let sel = app.marker_list_sel.min(all.len().saturating_sub(1));
                                if let Some(m) = all.get(sel) {
                                    use console_gis::geo::LatLon;
                                    app.map_centre = LatLon::new(m.lat, m.lon);
                                    app.navigate(View::Map);
                                }
                            }
                            KeyCode::Char('x') | KeyCode::Char('X') => {
                                if count > 0 { app.clearing_markers = true; }
                            }
                            _ => {}
                        }
                    }

                    View::ZoomExplorer => match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => app.navigate(View::Menu),
                        KeyCode::Up   | KeyCode::Char('+') | KeyCode::Char('=') => app.zoom_in(),
                        KeyCode::Down | KeyCode::Char('-') => app.zoom_out(),
                        _ => {}
                    },

                    View::Diagnostics => match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => app.navigate(View::Menu),
                        _ => {}
                    },
                }
            }
        }

        // ── Animation tick ────────────────────────────────────────────────────
        if last_tick.elapsed() >= tick_rate {
            let dt = last_tick.elapsed().as_secs_f64();
            app.tick(dt);
            last_tick = Instant::now();
        }

        if app.view == View::Splash && splash_start.elapsed() >= Duration::from_secs(3) {
            app.navigate(View::Menu);
        }
    }
}

// ── Globe key handler ─────────────────────────────────────────────────────────

fn handle_globe_keys(app: &mut App, code: KeyCode) {
    if app.placing_marker {
        match code {
            KeyCode::Esc => { app.placing_marker = false; }
            KeyCode::Enter => {
                // Transition from crosshair → symbol/label input
                let lat = app.globe_cursor.lat;
                let lon = app.globe_cursor.lon;
                app.marker_input = Some(MarkerInput {
                    lat, lon,
                    symbol_buf: String::new(),
                    label_buf:  String::new(),
                    blink:      false,
                    step:       MarkerInputStep::Symbol,
                    edit_id:    None,
                });
                app.placing_marker = false;
            }
            KeyCode::Left  | KeyCode::Char('a') => { app.globe.rot_y -= 0.05; }
            KeyCode::Right | KeyCode::Char('d') => { app.globe.rot_y += 0.05; }
            KeyCode::Up    => { app.globe.rot_x -= 0.05; }
            KeyCode::Down  => { app.globe.rot_x += 0.05; }
            _ => {}
        }
    } else {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => { app.navigate(View::Menu); }
            KeyCode::Left  | KeyCode::Char('a') => { app.globe.rot_y -= 0.08; app.animating = false; }
            KeyCode::Right | KeyCode::Char('d') => { app.globe.rot_y += 0.08; app.animating = false; }
            KeyCode::Up => {
                app.globe.rot_x = (app.globe.rot_x - 0.08)
                    .clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
                app.animating = false;
            }
            KeyCode::Down => {
                app.globe.rot_x = (app.globe.rot_x + 0.08)
                    .clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
                app.animating = false;
            }
            KeyCode::Char('w') | KeyCode::Char('W') => { app.globe_zoom_in(); }
            KeyCode::Char('s') | KeyCode::Char('S') => { app.globe_zoom_out(); }
            KeyCode::Char(' ') => { app.animating = !app.animating; }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                app.animating = false;
                app.placing_marker = true;
            }
            KeyCode::Char('i') | KeyCode::Char('I') => {
                app.importing = true;
                app.import_buf.clear();
                app.import_error = None;
            }
            KeyCode::Char('b') | KeyCode::Char('B') => {
                app.bookmarking = true;
                app.bookmark_buf.clear();
            }
            KeyCode::Char('x') | KeyCode::Char('X') => {
                if app.markers.count() > 0 { app.clearing_markers = true; }
            }
            _ => {}
        }
    }
}

// ── Map key handler ───────────────────────────────────────────────────────────

fn handle_map_keys(app: &mut App, code: KeyCode) {
    if app.placing_marker {
        match code {
            KeyCode::Esc => { app.placing_marker = false; }
            KeyCode::Enter => {
                let lat = app.map_centre.lat;
                let lon = app.map_centre.lon;
                app.marker_input = Some(MarkerInput {
                    lat, lon,
                    symbol_buf: String::new(),
                    label_buf:  String::new(),
                    blink:      false,
                    step:       MarkerInputStep::Symbol,
                    edit_id:    None,
                });
                app.placing_marker = false;
            }
            KeyCode::Left  | KeyCode::Char('a') => { app.pan(0.0, -pan_lon_step(app.zoom)); }
            KeyCode::Right | KeyCode::Char('d') => { app.pan(0.0,  pan_lon_step(app.zoom)); }
            KeyCode::Up    => { app.pan( pan_lat_step(app.zoom), 0.0); }
            KeyCode::Down  => { app.pan(-pan_lat_step(app.zoom), 0.0); }
            _ => {}
        }
    } else {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => { app.navigate(View::Menu); }
            KeyCode::Left  | KeyCode::Char('a') => { app.pan(0.0, -pan_lon_step(app.zoom)); }
            KeyCode::Right | KeyCode::Char('d') => { app.pan(0.0,  pan_lon_step(app.zoom)); }
            KeyCode::Up    => { app.pan( pan_lat_step(app.zoom), 0.0); }
            KeyCode::Down  => { app.pan(-pan_lat_step(app.zoom), 0.0); }
            KeyCode::Char('w') | KeyCode::Char('W')
            | KeyCode::Char('+') | KeyCode::Char('=') => { app.zoom_in(); }
            KeyCode::Char('s') | KeyCode::Char('S')
            | KeyCode::Char('-') => { app.zoom_out(); }
            KeyCode::Char('m') | KeyCode::Char('M') => { app.placing_marker = true; }
            KeyCode::Char('i') | KeyCode::Char('I') => {
                app.importing = true;
                app.import_buf.clear();
                app.import_error = None;
            }
            KeyCode::Char('b') | KeyCode::Char('B') => {
                app.bookmarking = true;
                app.bookmark_buf.clear();
            }
            KeyCode::Char('x') | KeyCode::Char('X') => {
                if app.markers.count() > 0 { app.clearing_markers = true; }
            }
            _ => {}
        }
    }
}

// ── Marker input step machine ─────────────────────────────────────────────────

fn handle_marker_input(app: &mut App, code: KeyCode) {
    let mi = match app.marker_input.as_mut() {
        Some(m) => m,
        None    => return,
    };

    match mi.step {
        MarkerInputStep::Symbol => match code {
            KeyCode::Esc => { app.marker_input = None; }
            KeyCode::Tab | KeyCode::Enter if mi.symbol_buf.is_empty() => {
                mi.symbol_buf = "●".to_string();
                mi.step = MarkerInputStep::Label;
            }
            KeyCode::Enter => { mi.step = MarkerInputStep::Label; }
            KeyCode::Backspace => { mi.symbol_buf.clear(); }
            KeyCode::Char(c) if !c.is_control() => {
                // Symbol is always exactly one grapheme — overwrite on each keypress
                mi.symbol_buf = c.to_string();
                mi.step = MarkerInputStep::Label; // auto-advance
            }
            _ => {}
        },
        MarkerInputStep::Label => match code {
            KeyCode::Esc => { app.marker_input = None; }
            KeyCode::Tab => {
                if mi.label_buf.is_empty() { mi.label_buf = "Marker".to_string(); }
            }
            KeyCode::Backspace => { mi.label_buf.pop(); }
            KeyCode::Enter => { mi.step = MarkerInputStep::Blink; }
            KeyCode::Char(c) if !c.is_control() => { mi.label_buf.push(c); }
            _ => {}
        },
        MarkerInputStep::Blink => {
            match code {
                KeyCode::Esc => { app.marker_input = None; return; }
                KeyCode::Char('y') | KeyCode::Char('Y') => { mi.blink = true; }
                _ => { mi.blink = false; } // N, Enter, anything else = no blink
            }
            // Commit the marker
            commit_marker_input(app);
        }
    }
}

fn commit_marker_input(app: &mut App) {
    let mi = match app.marker_input.take() {
        Some(m) => m,
        None    => return,
    };
    let symbol = if mi.symbol_buf.is_empty() { "●".to_string() } else { mi.symbol_buf };
    let label  = if mi.label_buf.is_empty()  { "Marker".to_string() } else { mi.label_buf };

    if let Some(id) = mi.edit_id {
        // Update existing marker
        if let Ok(Some(mut m)) = app.markers.get(id) {
            m.symbol = symbol;
            m.label  = label;
            m.blink  = mi.blink;
            let _ = app.markers.update(&m);
        }
    } else {
        // Insert new marker
        let _ = app.markers.insert_with_blink(mi.lat, mi.lon, symbol, label, mi.blink);
    }
}
