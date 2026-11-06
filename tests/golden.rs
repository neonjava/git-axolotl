use chrono::NaiveDate;
use git_axolotl::coat::{parse_hex_color, AxolotlCoat};
use git_axolotl::history::{
    summarize_calendar_window, AccountSnapshot, ContributionDay, ContributionWindow, LanguageShare,
    YearTotal,
};
use git_axolotl::output::{encode_gif, encode_png, encode_share_card_png};
use git_axolotl::render::{
    language_percent_label, render_share_card_frame, render_still_frame, render_swim_frame,
    render_swim_loop, AxolotlBiology, CameraView, FRAME_HEIGHT_PIXELS, FRAME_WIDTH_PIXELS,
    LOOP_FRAME_COUNT,
};

fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn demo_snapshot(commits: u64) -> AccountSnapshot {
    AccountSnapshot {
        login: "test-user".to_owned(),
        account_id: 42,
        observed_on: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
        total_contributions: commits,
        monthly_contributions: commits,
        current_streak: 7,
        longest_streak: 21,
        languages: vec![
            LanguageShare {
                name: "Rust".to_owned(),
                hex_color: "#dea584".to_owned(),
                weight: 10,
            },
            LanguageShare {
                name: "TypeScript".to_owned(),
                hex_color: "#3178c6".to_owned(),
                weight: 6,
            },
        ],
        lifetime_contributions: 0,
        lifetime_private_contributions: 0,
        yearly_contributions: Vec::new(),
    }
}

#[test]
fn stage_boundaries_follow_metamorphosis() {
    let label = |commits| AxolotlBiology::from_commits(commits).hud_stage_label();
    assert_eq!(label(0), "STAGE 1: HATCHLING");
    assert_eq!(label(149), "STAGE 1: HATCHLING");
    assert_eq!(label(150), "STAGE 2: LARVA");
    assert_eq!(label(499), "STAGE 2: LARVA");
    assert_eq!(label(500), "STAGE 3: JUVENILE");
    assert_eq!(label(1200), "STAGE 4: ADULT");
    assert_eq!(label(2500), "STAGE 5: ELDER");

    let hatchling = AxolotlBiology::from_commits(0);
    let larva = AxolotlBiology::from_commits(150);
    let juvenile = AxolotlBiology::from_commits(500);
    let elder = AxolotlBiology::from_commits(2800);
    assert_eq!(hatchling.front_legs, 0.0);
    assert!(larva.front_legs > 0.9 && larva.back_legs < 0.05);
    assert!(juvenile.back_legs > 0.9);
    assert!(elder.body_len > hatchling.body_len);
    assert!(elder.gill_fronds > hatchling.gill_fronds);
}

#[test]
fn annual_vs_rolling_365_window_summary() {
    let observed = NaiveDate::from_ymd_opt(2026, 2, 10).unwrap();
    let days = vec![
        ContributionDay {
            date: NaiveDate::from_ymd_opt(2025, 11, 15).unwrap(),
            count: 400,
        },
        ContributionDay {
            date: NaiveDate::from_ymd_opt(2026, 1, 20).unwrap(),
            count: 120,
        },
        ContributionDay {
            date: NaiveDate::from_ymd_opt(2026, 2, 10).unwrap(),
            count: 30,
        },
    ];

    let (annual_total, annual_month, _, _) =
        summarize_calendar_window(&days, observed, ContributionWindow::Annual);
    assert_eq!(annual_total, 150);
    assert_eq!(annual_month, 30);

    let (rolling_total, _, _, _) =
        summarize_calendar_window(&days, observed, ContributionWindow::Rolling365);
    assert_eq!(rolling_total, 550);
}

#[test]
fn hex_color_parser_handles_valid_and_invalid_strings() {
    assert_eq!(parse_hex_color("#dea584"), Some([222, 165, 132]));
    assert_eq!(parse_hex_color("3178c6"), Some([49, 120, 198]));
    assert_eq!(parse_hex_color("#123"), None);
    assert_eq!(parse_hex_color("1\u{e9}234x"), None);
}

/// Update this only after checking the new artwork by eye.
const GOLDEN_RGBA_HASH: u64 = 0xc7482db3cd3b37cb;

#[test]
fn still_frame_matches_pinned_golden_hash() {
    let snap = demo_snapshot(1250);
    let coat = AxolotlCoat::from_snapshot(&snap, false);
    let frame = render_still_frame(&snap, &coat, CameraView::Side);
    let hash = fnv1a_64(frame.as_rgba_bytes());
    assert_eq!(hash, GOLDEN_RGBA_HASH, "got 0x{hash:016x}");
}

#[test]
fn every_view_encodes_a_full_loop() {
    let snap = demo_snapshot(2800);
    let coat = AxolotlCoat::from_snapshot(&snap, false);
    for view in [CameraView::Side, CameraView::Dive, CameraView::Top] {
        let frames = render_swim_loop(&snap, &coat, view);
        assert_eq!(frames.len(), LOOP_FRAME_COUNT as usize);
        assert_ne!(frames[0], frames[12]);
        let gif_bytes = encode_gif(&frames).expect("encode_gif succeeds");
        assert!(gif_bytes.starts_with(b"GIF89a"));
    }
}

#[test]
fn loop_is_seamless_and_matches_single_frame_rendering() {
    let snap = demo_snapshot(800);
    let coat = AxolotlCoat::from_snapshot(&snap, false);
    let frames = render_swim_loop(&snap, &coat, CameraView::Side);
    let wrapped = render_swim_frame(&snap, &coat, CameraView::Side, LOOP_FRAME_COUNT);
    assert_eq!(frames[0], wrapped);
}

#[test]
fn each_account_gets_its_own_spot_pattern() {
    let first = demo_snapshot(1800);
    let mut second = demo_snapshot(1800);
    second.login = "someone-else".to_owned();
    second.account_id = 7;
    let coat = AxolotlCoat::from_snapshot(&first, false);
    assert_ne!(
        render_still_frame(&first, &coat, CameraView::Side),
        render_still_frame(&second, &coat, CameraView::Side)
    );
}

#[test]
fn lifetime_window_uses_every_year_and_annual_keeps_its_own_total() {
    let history = vec![
        YearTotal {
            year: 2023,
            count: 900,
        },
        YearTotal {
            year: 2021,
            count: 0,
        },
        YearTotal {
            year: 2026,
            count: 400,
        },
    ];

    let mut annual = demo_snapshot(400);
    annual.apply_history(history.clone(), 120, ContributionWindow::Annual);
    assert_eq!(annual.total_contributions, 400);
    assert_eq!(annual.lifetime_contributions, 1300);
    assert_eq!(annual.lifetime_private_contributions, 120);
    assert_eq!(annual.first_active_year(), Some(2023));

    let mut lifetime = demo_snapshot(400);
    lifetime.apply_history(history, 120, ContributionWindow::Lifetime);
    assert_eq!(lifetime.total_contributions, 1300);
}

#[test]
fn hud_shows_the_all_time_history() {
    let plain = demo_snapshot(400);
    let mut with_history = demo_snapshot(400);
    with_history.apply_history(
        vec![YearTotal {
            year: 2023,
            count: 900,
        }],
        0,
        ContributionWindow::Annual,
    );
    let coat = AxolotlCoat::from_snapshot(&plain, false);
    assert_ne!(
        render_still_frame(&plain, &coat, CameraView::Side),
        render_still_frame(&with_history, &coat, CameraView::Side)
    );
}

#[test]
fn old_snapshots_without_history_still_load() {
    let json = r#"{"login":"a","account_id":1,"observed_on":"2026-01-01","total_contributions":5,
        "monthly_contributions":5,"current_streak":0,"longest_streak":0,"languages":[]}"#;
    let snap: AccountSnapshot = serde_json::from_str(json).expect("old snapshot parses");
    assert_eq!(snap.lifetime_contributions, 0);
}

fn ten_language_snapshot() -> AccountSnapshot {
    let mut snap = demo_snapshot(1800);
    snap.languages = (0..12_u64)
        .map(|i| LanguageShare {
            name: format!("Lang{i}"),
            hex_color: format!("#{:02x}{:02x}80", 20 * i + 10, 240 - 20 * i),
            weight: 1000 - i * 80,
        })
        .collect();
    snap
}

#[test]
fn every_top_language_gets_a_tone_and_appears_on_the_axolotl() {
    let snap = ten_language_snapshot();
    let coat = AxolotlCoat::from_snapshot(&snap, false);
    assert_eq!(coat.tones.len(), 10);

    let frame = render_still_frame(&snap, &coat, CameraView::Side);
    let hud_top = (FRAME_HEIGHT_PIXELS - 38) as usize * FRAME_WIDTH_PIXELS as usize;
    let creature = &frame.rgba_pixels[..hud_top];
    let shown = coat
        .tones
        .iter()
        .skip(1)
        .filter(|tone| tone.ramp.iter().any(|shade| creature.contains(shade)))
        .count();
    assert_eq!(shown, 9, "every language after the first should be visible");
}

#[test]
fn natural_coat_uses_no_language_tones() {
    let snap = ten_language_snapshot();
    let coat = AxolotlCoat::from_snapshot(&snap, true);
    assert!(coat.tones.is_empty());
    assert_eq!(coat.frond_ramp(3), &coat.gill_tip);
    assert_eq!(coat.spot_ramp(0.5), &coat.markings);
}

#[test]
fn percent_labels_round_and_mark_tiny_shares() {
    assert_eq!(language_percent_label(41, 100), "41%");
    assert_eq!(language_percent_label(1, 1000), "<1%");
    assert_eq!(language_percent_label(5, 0), "500%");
}

#[test]
fn old_snapshots_with_repo_counts_still_load() {
    let json = r##"{"name":"Rust","hex_color":"#dea584","repo_count":3}"##;
    let lang: LanguageShare = serde_json::from_str(json).expect("old language parses");
    assert_eq!(lang.weight, 3);
}

#[test]
fn share_card_and_still_encode_as_png() {
    let snap = demo_snapshot(1250);
    let coat = AxolotlCoat::from_snapshot(&snap, true);
    let still = render_still_frame(&snap, &coat, CameraView::Top);
    let card = render_share_card_frame(&snap, &coat, CameraView::Side);
    let signature = b"\x89PNG\r\n\x1a\n";
    assert!(encode_png(&still).expect("png").starts_with(signature));
    assert!(encode_share_card_png(&card)
        .expect("share card")
        .starts_with(signature));
}
