use anyhow::Context;
use std::env;
use std::time::{Duration, Instant};

fn print_help() {
    println!(
        r#"LightConsole CLI

            Commands:
            new <show_name>
            add-fixture <show.json> <fixture_id> <name> <fixture_type> <universe> <address>
            list <show.json>
            save-default <show.json>
            load <show.json>

            Examples:
            cargo run -p console_cli -- new "My Show"
            cargo run -p console_cli -- save-default show.json
            cargo run -p console_cli -- add-fixture show.json 1 "PAR 1" rgb_par_3ch 1 1
            cargo run -p console_cli -- list show.json
        "#
    );
}

use std::io::{self, Write};

fn snapshot_fixture_values(
    show: &console_core::Show,
    playback: &console_core::Playback,
    programmer: &console_core::Programmer,
    fixture_id: u32,
) -> anyhow::Result<console_core::FixtureValues> {
    let tracked = playback.state_map(show)?;
    let base = tracked.get(&fixture_id);

    // Start from tracked values (None -> 0)
    let mut intensity = base.and_then(|v| v.intensity).unwrap_or(0);
    let mut r = base.and_then(|v| v.r).unwrap_or(0);
    let mut g = base.and_then(|v| v.g).unwrap_or(0);
    let mut b = base.and_then(|v| v.b).unwrap_or(0);

    // Apply programmer overlay
    if let Some(v) = programmer.intensity {
        intensity = v;
    }
    if let Some(v) = programmer.r {
        r = v;
    }
    if let Some(v) = programmer.g {
        g = v;
    }
    if let Some(v) = programmer.b {
        b = v;
    }

    Ok(console_core::FixtureValues {
        intensity: Some(intensity),
        r: Some(r),
        g: Some(g),
        b: Some(b),
    })
}

fn repl(show_path: &str) -> anyhow::Result<()> {
    let show = console_core::Show::load_json_file(show_path)?;
    let mut rt = console_core::Runtime::new(show);
    let mut active_pb: char = 'a';

    let mut running = false;
    let mut last_tick = Instant::now();
    let mut last_print = Instant::now();
    let print_every = Duration::from_millis(200); // adjust if you want

    rt.show
        .cue_lists
        .entry("main".to_string())
        .or_insert_with(console_core::CueList::default);

    println!("Loaded show: {}", rt.show.name);
    println!("Type 'help' for commands. 'quit' to exit.");

    let mut rec_fade_ms: u32 = 1000;
    let mut rec_delay_ms: u32 = 0;

    fn pb_mut<'a>(
        rt: &'a mut console_core::Runtime,
        active: char,
    ) -> &'a mut console_core::Playback {
        if active == 'b' {
            &mut rt.playback_b
        } else {
            &mut rt.playback_a
        }
    }

    fn pb_ref<'a>(rt: &'a console_core::Runtime, active: char) -> &'a console_core::Playback {
        if active == 'b' {
            &rt.playback_b
        } else {
            &rt.playback_a
        }
    }

    loop {
        print!("lc> ");
        io::stdout().flush()?;

        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            // EOF (Ctrl+D)
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        let cmd = parts[0].to_lowercase();

        if running {
            let now = Instant::now();
            let dt = now.duration_since(last_tick);
            last_tick = now;

            let ms = (dt.as_secs_f64() * 1000.0).round() as u32;
            if ms > 0 {
                rt.tick(ms.min(100)); // clamp so pauses don't jump too far
            }

            if now.duration_since(last_print) >= print_every {
                last_print = now;

                let live = rt.render()?;
                let nz = live.nonzero();

                println!(
                    "A: {:?} | B: {:?} | nz={}",
                    rt.playback_a.current,
                    rt.playback_b.current,
                    nz.len()
                );
            }
        }

        // before splitting into "cmd words"
        match console_core::progcmd::try_apply_programmer_line(line, &mut rt.programmer) {
            console_core::progcmd::ApplyStatus::Applied => {
                println!("(programmer) selection+values applied");
                continue;
            }
            console_core::progcmd::ApplyStatus::Incomplete => {
                println!("(programmer) incomplete input…");
                continue;
            }
            console_core::progcmd::ApplyStatus::NotProgrammer => {
                // fall through to existing commands (help, list, record, etc.)
            }
        }

        match cmd.as_str() {
            "help" => {
                println!(
                    r#"Commands:
                        select <id>
                        select <a> thru <b>
                        at <0..100>
                        rgb <0..255> <0..255> <0..255>
                        show
                        clear        (clears selection + values)
                        clearvals    (keeps selection, clears values)
                        clearprog    (clears programmer)
                        list         (lists fixtures from showfile)
                        record palette intensity <name>
                        record palette color <name>
                        palettes
                        apply palette <name>
                        record cue <number> <label...> [track|only]
                        update cue <number> [track|only]
                        delete cue <number>
                        pbmode tracking|cueonly
                        block <cue_number>
                        unblock <cue_number>
                        goto <cue_number>
                        go
                        state
                        out
                        pb a|b
                        run
                        stop
                        save
                        quit
                        "#
                );
            }
            "quit" | "exit" => break,

            "list" => {
                println!("Fixtures:");
                for f in rt.show.patch.list_fixtures() {
                    println!(
                        "  #{:>3} | {:<10} | type {:<12} | U{} @ {}",
                        f.fixture_id, f.name, f.fixture_type, f.universe, f.address
                    );
                }
            }

            "select" => {
                if parts.len() == 2 {
                    let id: u32 = parts[1].parse()?;
                    rt.programmer.select_one(id);
                } else if parts.len() == 4 && parts[2].eq_ignore_ascii_case("thru") {
                    let a: u32 = parts[1].parse()?;
                    let b: u32 = parts[3].parse()?;
                    rt.programmer.select_range(a, b);
                } else {
                    println!("Usage: select <id>  OR  select <a> thru <b>");
                }
            }

            "at" => {
                if parts.len() != 2 {
                    println!("Usage: at <0..100>");
                    continue;
                }
                let pct: u8 = parts[1].parse()?;
                rt.programmer.set_intensity_percent(pct);
            }

            "rgb" | "color" => {
                if parts.len() != 4 {
                    println!("Usage: rgb <r> <g> <b> (0..255)");
                    continue;
                }
                let r: u8 = parts[1].parse()?;
                let g: u8 = parts[2].parse()?;
                let b: u8 = parts[3].parse()?;
                rt.programmer.set_rgb(r, g, b);
            }

            "show" => {
                println!("Selected: {:?}", rt.programmer.selected);
                println!(
                    "Values: intensity={:?} rgb={:?}",
                    rt.programmer.intensity,
                    rt.programmer.r.zip(rt.programmer.g).zip(rt.programmer.b)
                );
            }

            "out" => {
                let live = rt.render()?;
                let nz = live.nonzero();

                println!(
                    "A cue: {:?} mode: {:?} | B cue: {:?} mode: {:?} | Selected: {:?}",
                    rt.playback_a.current,
                    rt.playback_a.mode,
                    rt.playback_b.current,
                    rt.playback_b.mode,
                    rt.programmer.selected
                );

                if nz.is_empty() {
                    println!("(all zeros)");
                    continue;
                }

                println!("Non-zero DMX output:");
                for (u, addr, v) in nz {
                    println!("  U{}:{:03} = {}", u, addr, v);
                }
            }

            "clear" => rt.programmer.clear_all(),
            "clearvals" => rt.programmer.clear_values(),
            "clearprog" => rt.programmer.clear_all(),
            "clearall" => {
                rt.programmer.clear_all();
                rt.programmer.selected.clear();
                println!("Cleared programmer + selection.");
            }

            "save" => {
                rt.show.save_json_file(show_path)?;
                println!("Saved showfile: {}", show_path);
            }

            "palettes" => {
                if rt.show.palettes.is_empty() {
                    println!("(no palettes yet)");
                    continue;
                }
                println!("Palettes:");
                for (name, pal) in &rt.show.palettes {
                    match pal.kind {
                        console_core::PaletteKind::Intensity => {
                            let v = pal.values.intensity.unwrap_or(0);
                            let pct = v as u16 * 100 / 255;
                            println!("  {name} | Intensity | {v} (~{pct}%)");
                        }
                        console_core::PaletteKind::Color => {
                            let r = pal.values.r.unwrap_or(0);
                            let g = pal.values.g.unwrap_or(0);
                            let b = pal.values.b.unwrap_or(0);
                            println!("  {name} | Color | rgb({r},{g},{b})");
                        }
                    }
                }
            }

            "record" => {
                // record group <name>
                if parts.len() == 3 && parts[1].eq_ignore_ascii_case("group") {
                    let name = parts[2].to_string();

                    if rt.programmer.selected.is_empty() {
                        println!("No fixtures selected.");
                        continue;
                    }

                    rt.show
                        .groups
                        .insert(name.clone(), rt.programmer.selected.clone());
                    rt.show.save_json_file(show_path)?;
                    println!("Recorded group '{name}' and saved.");
                    continue;
                }
                // record cue <number> <label...> [track|only]
                if parts.len() >= 3 && parts[1].eq_ignore_ascii_case("cue") {
                    let num: u32 = parts[2].parse()?;

                    // Parse optional mode at end
                    let mut mode = "track";
                    let mut end = parts.len();
                    if let Some(last) = parts.last()
                        && (last.eq_ignore_ascii_case("only") || last.eq_ignore_ascii_case("track"))
                    {
                        mode = last;
                        end -= 1;
                    }

                    let label = if end >= 4 {
                        parts[3..end].join(" ")
                    } else {
                        format!("Cue {num}")
                    };

                    if rt.programmer.selected.is_empty() {
                        println!("Nothing selected. Use: select ...");
                        continue;
                    }

                    let mut changes = std::collections::BTreeMap::new();

                    if mode.eq_ignore_ascii_case("track") {
                        // Track: record programmer deltas only
                        let delta = console_core::FixtureValues {
                            intensity: rt.programmer.intensity,
                            r: rt.programmer.r,
                            g: rt.programmer.g,
                            b: rt.programmer.b,
                        };

                        if delta.is_all_none() {
                            println!("No values in programmer to record. Use: at / rgb / r/g/b");
                            continue;
                        }

                        for &fid in &rt.programmer.selected {
                            changes.insert(fid, delta.clone());
                        }
                    } else {
                        // 1) compute snaps FIRST (immutable borrows only)
                        let snaps: Vec<(u32, console_core::FixtureValues)> = rt
                            .programmer
                            .selected
                            .iter()
                            .copied()
                            .map(|fid| {
                                let snap = snapshot_fixture_values(
                                    &rt.show,
                                    pb_ref(&rt, active_pb),
                                    &rt.programmer,
                                    fid,
                                )?;
                                Ok((fid, snap))
                            })
                            .collect::<anyhow::Result<_>>()?;

                        // 2) fill changes
                        for (fid, snap) in snaps {
                            changes.insert(fid, snap);
                        }
                    }

                    let cue = console_core::Cue {
                        number: num,
                        label,
                        block: false,
                        fade_ms: rec_fade_ms,
                        delay_ms: rec_delay_ms,
                        changes,
                    };

                    let cl = rt
                        .show
                        .cue_lists
                        .get_mut("main")
                        .expect("main cuelist exists");
                    cl.cues.insert(num, cue);

                    rt.show.save_json_file(show_path)?;
                    println!("Recorded cue {num} ({mode}) into cuelist 'main' and saved.");
                    continue;
                }

                // ... keep your existing `record palette ...` handling below
                println!(
                    "Usage: record cue <number> <label...> [track|only]  OR  record palette ..."
                );
            }

            "update" => {
                // update cue <number> [track|only]
                if parts.len() < 3 || !parts[1].eq_ignore_ascii_case("cue") {
                    println!("Usage: update cue <number> [track|only]");
                    continue;
                }
                let num: u32 = parts[2].parse()?;
                let mode = if parts.len() >= 4 { parts[3] } else { "track" };

                if rt.programmer.selected.is_empty() {
                    println!("Nothing selected. Use: select ...");
                    continue;
                }

                // ---- Phase 1: compute what we want to apply (NO mutable borrows of show) ----
                let snaps: Option<Vec<(u32, console_core::FixtureValues)>> =
                    if mode.eq_ignore_ascii_case("only") {
                        Some(
                            rt.programmer
                                .selected
                                .iter()
                                .copied()
                                .map(|fid| {
                                    let snap = snapshot_fixture_values(
                                        &rt.show,
                                        pb_ref(&rt, active_pb),
                                        &rt.programmer,
                                        fid,
                                    )?;
                                    Ok((fid, snap))
                                })
                                .collect::<anyhow::Result<_>>()?,
                        )
                    } else {
                        None
                    };

                let delta: Option<console_core::FixtureValues> =
                    if mode.eq_ignore_ascii_case("track") {
                        let d = console_core::FixtureValues {
                            intensity: rt.programmer.intensity,
                            r: rt.programmer.r,
                            g: rt.programmer.g,
                            b: rt.programmer.b,
                        };
                        if d.is_all_none() {
                            println!("No values in programmer to update. Use: at / rgb / r/g/b");
                            continue;
                        }
                        Some(d)
                    } else {
                        None
                    };

                if !(mode.eq_ignore_ascii_case("track") || mode.eq_ignore_ascii_case("only")) {
                    println!("Unknown mode '{mode}'. Use track|only");
                    continue;
                }

                // ---- Phase 2: mutate the cue (NOW we can borrow show mutably) ----
                let cl = rt
                    .show
                    .cue_lists
                    .get_mut("main")
                    .expect("main cuelist exists");

                let cue = match cl.cues.get_mut(&num) {
                    Some(c) => c,
                    None => {
                        println!("Cue {num} not found. Type: cues");
                        continue;
                    }
                };

                if let Some(d) = delta {
                    for &fid in &rt.programmer.selected {
                        cue.changes.insert(fid, d.clone());
                    }
                }

                if let Some(list) = snaps {
                    for (fid, snap) in list {
                        cue.changes.insert(fid, snap);
                    }
                }

                rt.show.save_json_file(show_path)?;
                println!("Updated cue {num} ({mode}) for selected fixtures and saved.");
            }

            "delete" => {
                if parts.len() < 3 {
                    println!(
                        "Usage: delete cue <num> | delete group <name...> | delete palette <name...>"
                    );
                    continue;
                }

                match parts[1].to_lowercase().as_str() {
                    "cue" => {
                        if parts.len() != 3 {
                            println!("Usage: delete cue <num>");
                            continue;
                        }
                        let num: u32 = parts[2].parse()?;

                        let cl = rt
                            .show
                            .cue_lists
                            .get_mut("main")
                            .expect("main cuelist exists");

                        if cl.cues.remove(&num).is_none() {
                            println!("Unknown cue {num}");
                            continue;
                        }

                        // Guard rail: if A/B were on this cue, clear them
                        rt.playback_a.on_cue_deleted(num);
                        rt.playback_b.on_cue_deleted(num);

                        rt.show.save_json_file(show_path)?;
                        println!("Deleted cue {num} and saved.");
                    }

                    "group" => {
                        let name = parts[2..].join(" ");
                        if rt.show.groups.remove(&name).is_none() {
                            println!("Unknown group '{name}'");
                            continue;
                        }
                        rt.show.save_json_file(show_path)?;
                        println!("Deleted group '{name}' and saved.");
                    }

                    "palette" => {
                        let name = parts[2..].join(" ");

                        // Most likely your show stores palettes in a map keyed by name.
                        // If your field name differs, use `rg "palettes"` in console_core to confirm.
                        if rt.show.palettes.remove(&name).is_none() {
                            println!("Unknown palette '{name}'");
                            continue;
                        }

                        rt.show.save_json_file(show_path)?;
                        println!("Deleted palette '{name}' and saved.");
                    }

                    _ => {
                        println!(
                            "Usage: delete cue <num> | delete group <name...> | delete palette <name...>"
                        );
                    }
                }
            }

            "apply" => {
                // apply palette <name>
                if parts.len() != 3 || !parts[1].eq_ignore_ascii_case("palette") {
                    println!("Usage: apply palette <name>");
                    continue;
                }
                let name = parts[2];
                let pal = match rt.show.palettes.get(name) {
                    Some(p) => p,
                    None => {
                        println!("Unknown palette '{name}'. Type: palettes");
                        continue;
                    }
                };
                rt.programmer.apply_palette(pal);
                println!("Applied palette '{name}' to programmer.");
            }

            "cues" => {
                let cl = rt.show.cue_lists.get("main").unwrap();
                if cl.cues.is_empty() {
                    println!("(no cues yet)");
                    continue;
                }
                println!(
                    "Cuelist: main | A current: {:?} | B current: {:?} | active: {}",
                    rt.playback_a.current,
                    rt.playback_b.current,
                    active_pb.to_ascii_uppercase()
                );
                for (&num, cue) in &cl.cues {
                    let cur = pb_ref(&rt, active_pb).current;
                    let mark = if Some(num) == cur { " <==" } else { "" };
                    println!(
                        "  {} | {} | fade={}ms delay={}ms block={}{}",
                        num, cue.label, cue.fade_ms, cue.delay_ms, cue.block, mark
                    );
                }
            }

            "goto" => {
                if parts.len() != 2 {
                    println!("Usage: goto <cue_number>");
                    continue;
                }
                let num: u32 = parts[1].parse()?;

                let cur = match active_pb {
                    'b' => {
                        rt.playback_b.goto(&rt.show, num)?;
                        rt.playback_b.current
                    }
                    _ => {
                        rt.playback_a.goto(&rt.show, num)?;
                        rt.playback_a.current
                    }
                };

                println!(
                    "Playback {} now at cue {:?}",
                    active_pb.to_ascii_uppercase(),
                    cur
                );
            }

            "go" => {
                let cur = match active_pb {
                    'b' => rt.playback_b.go(&rt.show)?,
                    _ => rt.playback_a.go(&rt.show)?,
                };

                println!(
                    "Playback {} now at cue {:?}",
                    active_pb.to_ascii_uppercase(),
                    cur
                );
            }

            "tick" => {
                if parts.len() != 2 {
                    println!("Usage: tick <ms>");
                    continue;
                }
                let ms: u32 = parts[1].parse()?;
                rt.tick(ms);
                println!("Ticked {ms}ms");
            }

            "time" => {
                if parts.len() < 2 || parts.len() > 3 {
                    println!("Usage: time <fade_ms> [delay_ms]");
                    continue;
                }
                rec_fade_ms = parts[1].parse()?;
                rec_delay_ms = if parts.len() == 3 {
                    parts[2].parse()?
                } else {
                    0
                };
                println!("Record defaults: fade_ms={rec_fade_ms} delay_ms={rec_delay_ms}");
            }

            "pbmode" => {
                if parts.len() != 2 {
                    println!("Usage: pbmode tracking|cueonly");
                    continue;
                }
                let pb = pb_mut(&mut rt, active_pb);
                match parts[1].to_lowercase().as_str() {
                    "tracking" => pb.mode = console_core::PlaybackMode::Tracking,
                    "cueonly" => pb.mode = console_core::PlaybackMode::CueOnly,
                    _ => {
                        println!("Usage: pbmode tracking|cueonly");
                        continue;
                    }
                }
                println!(
                    "Playback {} mode set to {:?}",
                    active_pb.to_ascii_uppercase(),
                    pb_ref(&rt, active_pb).mode
                );
            }

            "block" | "unblock" => {
                if parts.len() != 2 {
                    println!("Usage: block <cue_number>  OR  unblock <cue_number>");
                    continue;
                }
                let num: u32 = parts[1].parse()?;

                // Do the mutation inside a small scope so the mutable borrow ends
                let new_value = cmd == "block";
                let result: Option<bool> = {
                    let cl = rt
                        .show
                        .cue_lists
                        .get_mut("main")
                        .expect("main cuelist exists");
                    match cl.cues.get_mut(&num) {
                        Some(cue) => {
                            cue.block = new_value;
                            Some(cue.block)
                        }
                        None => None,
                    }
                };
                match result {
                    Some(v) => {
                        rt.show.save_json_file(show_path)?;
                        println!("Cue {num} block = {v}");
                    }
                    None => println!("Cue {num} not found. Type: cues"),
                }
            }
            "state" => {
                let pb = pb_ref(&rt, active_pb);
                let st = pb.state_map(&rt.show)?;
                if st.is_empty() {
                    println!("(empty state)");
                    continue;
                }

                println!(
                    "Playback {} cue: {:?} mode: {:?}",
                    active_pb.to_ascii_uppercase(),
                    pb.current,
                    pb.mode
                );
                for (fid, v) in st {
                    println!(
                        "  Fixture {:>3}: I={:?} RGB=({:?},{:?},{:?})",
                        fid, v.intensity, v.r, v.g, v.b
                    );
                }
            }

            "pb" => {
                if parts.len() != 2 {
                    println!("Usage: pb a|b");
                    continue;
                }
                match parts[1].to_lowercase().as_str() {
                    "a" => active_pb = 'a',
                    "b" => active_pb = 'b',
                    _ => {
                        println!("Usage: pb a|b");
                        continue;
                    }
                }
                println!("Active playback = {}", active_pb.to_ascii_uppercase());
            }

            "trans" => match pb_ref(&rt, active_pb).transition_info() {
                Some((elapsed, delay, fade)) => {
                    println!("Transition: elapsed={elapsed}ms delay={delay}ms fade={fade}ms");
                }
                None => println!("Transition: (none)"),
            },

            "r" => {
                if parts.len() != 2 {
                    println!("Usage: r <0..255>");
                    continue;
                }
                rt.programmer.r = Some(parts[1].parse()?);
            }
            "g" => {
                if parts.len() != 2 {
                    println!("Usage: g <0..255>");
                    continue;
                }
                rt.programmer.g = Some(parts[1].parse()?);
            }
            "b" => {
                if parts.len() != 2 {
                    println!("Usage: b <0..255>");
                    continue;
                }
                rt.programmer.b = Some(parts[1].parse()?);
            }

            "run" => {
                running = true;
                last_tick = Instant::now();
                last_print = Instant::now();

                let live = rt.render()?;
                let nz = live.nonzero();

                println!(
                    "A:{:?}({:?}) B:{:?}({:?}) active:{} nz={}",
                    rt.playback_a.current,
                    rt.playback_a.mode,
                    rt.playback_b.current,
                    rt.playback_b.mode,
                    active_pb.to_ascii_uppercase(),
                    nz.len()
                );
            }

            "stop" => {
                running = false;
                println!("Run mode: OFF");
            }

            "groups" => {
                if rt.show.groups.is_empty() {
                    println!("(no groups)");
                    continue;
                }
                println!("Groups:");
                for (name, set) in &rt.show.groups {
                    let ids = set
                        .iter()
                        .map(|n| n.to_string())
                        .collect::<Vec<_>>()
                        .join(" ");
                    println!("  {name} | {ids}");
                }
            }

            "group" => {
                if parts.len() != 2 {
                    println!("Usage: group <name>");
                    continue;
                }
                let name = parts[1];

                let Some(sel) = rt.show.groups.get(name) else {
                    println!("Unknown group '{name}'");
                    continue;
                };

                rt.programmer.selected = sel.clone();
                println!("Selected group '{name}'");
            }

            _ => println!("Unknown command. Type 'help'."),
        }
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_help();
        return Ok(());
    }

    match args[1].as_str() {
        "new" => {
            let name = args.get(2).context("missing <show_name>")?;
            let mut show = console_core::Show::new(name);

            // Add default fixture types to the show.
            for ft in console_core::default_fixture_types() {
                show.patch.add_fixture_type(ft);
            }

            println!("Created show in memory: {}", show.name);
            println!("Tip: run `save-default <file.json>` to write a showfile.");
        }
        "save-default" => {
            let path = args.get(2).context("missing <show.json>")?;
            let mut show = console_core::Show::new("Default Show");

            for ft in console_core::default_fixture_types() {
                show.patch.add_fixture_type(ft);
            }

            show.save_json_file(path)?;
            println!("Saved default showfile to: {}", path);
        }
        "load" => {
            let path = args.get(2).context("missing <show.json>")?;
            let show = console_core::Show::load_json_file(path)?;
            println!("Loaded show: {}", show.name);
            println!(
                "Fixture types: {}, fixtures: {}",
                show.patch.fixture_types.len(),
                show.patch.fixtures.len()
            );
        }
        "add-fixture" => {
            let path = args.get(2).context("missing <show.json>")?;
            let fixture_id: u32 = args
                .get(3)
                .context("missing <fixture_id>")?
                .parse()
                .context("fixture_id must be a number")?;
            let name = args.get(4).context("missing <name>")?;
            let fixture_type = args.get(5).context("missing <fixture_type>")?;
            let universe: u16 = args
                .get(6)
                .context("missing <universe>")?
                .parse()
                .context("universe must be a number")?;
            let address: u16 = args
                .get(7)
                .context("missing <address>")?
                .parse()
                .context("address must be a number")?;

            let mut show = console_core::Show::load_json_file(path)
                .with_context(|| format!("failed to load showfile '{path}'"))?;

            let fixture = console_core::FixtureInstance::new(
                fixture_id,
                name,
                fixture_type,
                universe,
                address,
            );

            show.patch.add_fixture(fixture)?;
            show.save_json_file(path)?;
            println!("Added fixture {} and saved {}", fixture_id, path);
        }
        "list" => {
            let path = args.get(2).context("missing <show.json>")?;
            let show = console_core::Show::load_json_file(path)?;
            println!("Show: {}", show.name);
            println!("Fixtures:");
            for f in show.patch.list_fixtures() {
                println!(
                    "  #{:>3} | {:<10} | type {:<12} | U{} @ {}",
                    f.fixture_id, f.name, f.fixture_type, f.universe, f.address
                );
            }
        }
        "repl" => {
            let path = args.get(2).context("missing <show.json>")?;
            repl(path)?;
        }

        _ => print_help(),
    }

    Ok(())
}
