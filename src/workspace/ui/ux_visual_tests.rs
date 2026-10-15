// Actual shipping renderer, invented data. No owner, provider, Git scan or PTY
// is attached; exporting frames never upgrades the design into runtime proof.
mod ux_v15_native {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    fn fixture(scene: &str, theme: aw::config::ThemeMode) -> Ui {
        let mut ui = model();
        ui.ui_config.theme = theme;
        ui.snapshot.session_title = Some("UX fixture".into());
        ui.snapshot.status = "Invented visual fixture · no execution".into();
        ui.snapshot.turns = Arc::new(vec![turn_with_trace(
            "Review the exact source.\nUnicode: 界 · cafe\u{301} · 👩‍💻",
            "This is the existing native Agent projection.\n\nNo provider or Tool was invoked.",
            aw::conversation::Outcome::Completed,
            None,
            None,
        )]);
        ui.files.insert(
            "src/fixture.rs".into(),
            "// Invented, read-only visual fixture\nfn main() {\n    println!(\"界 · café\");\n}\n"
                .into(),
        );
        ui.file_lines.insert(
            "src/fixture.rs".into(),
            Arc::new(
                ui.files["src/fixture.rs"]
                    .lines()
                    .map(str::to_owned)
                    .collect(),
            ),
        );
        match scene {
            "agent" => {}
            "file" => ui.open(Target::File {
                path: "src/fixture.rs".into(),
            }),
            "palette" => ui.open_palette(),
            "tabs-overflow" => {
                ui.open(Target::Help);
                ui.open(Target::Settings);
                for path in ["src/fixture.rs", "crates/very-long-unicode-界/file.rs"] {
                    ui.open(Target::File { path: path.into() });
                }
            }
            "split" => {
                ui.apply(Action::SplitGroup {
                    pane: ui.group(),
                    side: Side::Right,
                });
                ui.open(Target::File {
                    path: "src/fixture.rs".into(),
                });
            }
            "stale-terminal" => ui.open(Target::Terminal {
                terminal: "absent-fixture".into(),
            }),
            _ => panic!("unknown visual fixture"),
        }
        ui
    }

    fn text(buffer: &ratatui::buffer::Buffer) -> String {
        (0..buffer.area.height)
            .map(|y| {
                let mut row = String::new();
                let mut next = 0;
                for x in 0..buffer.area.width {
                    if x < next {
                        continue;
                    }
                    let symbol = buffer[(x, y)].symbol();
                    row.push_str(symbol);
                    next = x.saturating_add(UnicodeWidthStr::width(symbol).max(1) as u16);
                }
                row
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn ux_v15_native_gallery_preserves_identity_and_resize_state() {
        let output = std::env::var_os("AW_UX_NATIVE_OUTPUT").map(PathBuf::from);
        if let Some(path) = &output {
            std::fs::create_dir_all(path).unwrap();
        }
        let mut manifest = Vec::new();
        for (theme_name, theme) in [
            ("dark", aw::config::ThemeMode::Dark),
            ("light", aw::config::ThemeMode::Light),
            ("system", aw::config::ThemeMode::System),
        ] {
            for scene in [
                "agent",
                "file",
                "palette",
                "tabs-overflow",
                "split",
                "stale-terminal",
            ] {
                let mut ui = fixture(scene, theme);
                let layout = ui.layout.clone();
                let draft = ui.draft.text().to_owned();
                let turns = ui.snapshot.turns.clone();
                assert!(ui.terminal_pool.is_none());
                assert!(ui.jobs.is_empty());
                for (width, height) in [(80, 24), (100, 30), (120, 40), (160, 45), (200, 60)] {
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    terminal.draw(|frame| ui.draw(frame)).unwrap();
                    let buffer = terminal.backend().buffer();
                    let painted = text(buffer);
                    assert_eq!(painted.lines().count(), usize::from(height));
                    assert!(painted
                        .lines()
                        .all(|row| UnicodeWidthStr::width(row) == usize::from(width)));
                    assert_eq!(ui.layout, layout, "paint/resize changed target identity");
                    assert_eq!(ui.draft.text(), draft);
                    assert!(Arc::ptr_eq(&ui.snapshot.turns, &turns));
                    assert!(ui.jobs.is_empty(), "paint scheduled background work");
                    for region in &ui.hit_regions {
                        assert!(region.area.right() <= width && region.area.bottom() <= height);
                    }
                    if scene == "palette" {
                        assert!(painted.contains("Command Palette"));
                    }
                    if matches!(scene, "file" | "split") {
                        assert!(painted.contains("fn main()"));
                    }
                    if scene == "stale-terminal" {
                        assert!(painted.contains("Terminal not live in this owner"));
                    }
                    let name = format!("{scene}-{theme_name}-{width}x{height}");
                    let cells = buffer
                        .content
                        .iter()
                        .map(|cell| {
                            (
                                cell.symbol(),
                                format!("{:?}", cell.fg),
                                format!("{:?}", cell.bg),
                                format!("{:?}", cell.modifier),
                                UnicodeWidthStr::width(cell.symbol()),
                            )
                        })
                        .collect::<Vec<_>>();
                    let hits = ui
                        .hit_regions
                        .iter()
                        .map(|region| {
                            serde_json::json!({
                                "x": region.area.x, "y": region.area.y,
                                "width": region.area.width, "height": region.area.height,
                                "target": format!("{:?}", region.hit),
                            })
                        })
                        .collect::<Vec<_>>();
                    if let Some(path) = &output {
                        std::fs::write(path.join(format!("{name}.txt")), format!("{painted}\n"))
                            .unwrap();
                        let frame = serde_json::json!({
                            "schema": "aw-native-ui-proof-v1", "scene": scene, "theme": theme_name,
                            "width": width, "height": height, "cells": cells, "hits": hits,
                            "evidence": "shipping ratatui Ui::draw/TestBackend; invented data; no live owner, provider or PTY",
                        });
                        std::fs::write(
                            path.join(format!("{name}.json")),
                            serde_json::to_vec(&frame).unwrap(),
                        )
                        .unwrap();
                    }
                    manifest.push(name);
                }
                // Return to the initial size on the SAME Ui, with no lost focus,
                // layout, input draft or accepted conversation reference.
                let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
                terminal.draw(|frame| ui.draw(frame)).unwrap();
                assert_eq!(ui.layout, layout);
                assert_eq!(ui.draft.text(), draft);
                assert!(Arc::ptr_eq(&ui.snapshot.turns, &turns));
            }
        }
        assert_eq!(manifest.len(), 90);
        if let Some(path) = &output {
            std::fs::write(
                path.join("manifest.json"),
                serde_json::to_vec_pretty(&manifest).unwrap(),
            )
            .unwrap();
        }
    }
}
