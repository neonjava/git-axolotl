use chrono::NaiveDate;
use git_axolotl::coat::{parse_hex_color, AxolotlCoat};
use git_axolotl::history::{
    summarize_calendar_window, AccountSnapshot, ContributionDay, ContributionWindow, LanguageShare,
};
use git_axolotl::output::{encode_gif, encode_png, encode_share_card_png};
use git_axolotl::render::{
    render_share_card_frame, render_still_frame, render_swim_frame, CameraView, AxolotlBiology,
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
                repo_count: 10,
            },
            LanguageShare {
                name: "TypeScript".to_owned(),
                hex_color: "#3178c6".to_owned(),
                repo_count: 6,
            },
        ],
    }
}

#[test]
fn stage_boundaries_cover_zero_to_bull_log_scale() {
    let s0 = AxolotlBiology::from_commits(0);
    assert_eq!(s0.stage_name(), "Baby Axolotl Calf");
    assert_eq!(s0.hud_stage_label(), "STAGE 1: BABY CALF");

    let s1 = AxolotlBiology::from_commits(149);
    assert_eq!(s1.hud_stage_label(), "STAGE 1: BABY CALF");

    let s2 = AxolotlBiology::from_commits(150);
    assert_eq!(s2.hud_stage_label(), "STAGE 2: YEARLING CALF");

    let s3 = AxolotlBiology::from_commits(500);
    assert_eq!(s3.hud_stage_label(), "STAGE 3: ADOLESCENT");

    let s4 = AxolotlBiology::from_commits(1200);
    assert_eq!(s4.hud_stage_label(), "STAGE 4: MATURE AXOLOTL");

    let s5 = AxolotlBiology::from_commits(2500);
    assert_eq!(s5.hud_stage_label(), "STAGE 5: ADULT BULL");
    assert!(s5.body_len > s0.body_len);
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
}

#[test]
fn deterministic_golden_hash_for_still_png_and_share_card() {
    let snap = demo_snapshot(1250);
    let coat = AxolotlCoat::from_snapshot(&snap, false);

    let frame_a = render_still_frame(&snap, &coat, CameraView::Side);
    let frame_b = render_still_frame(&snap, &coat, CameraView::Side);
    assert_eq!(frame_a, frame_b);

    let png_a = encode_png(&frame_a).expect("encode_png succeeds");
    let png_b = encode_png(&frame_b).expect("encode_png succeeds");
    assert_eq!(fnv1a_64(&png_a), fnv1a_64(&png_b));
    let rgba_hash = fnv1a_64(frame_a.as_rgba_bytes());
    println!("GOLDEN_RGBA_HASH=0x{rgba_hash:016x}");
    assert_eq!(rgba_hash, fnv1a_64(frame_b.as_rgba_bytes()));

    let swim_0 = render_swim_frame(&snap, &coat, CameraView::Side, 0);
    let swim_1 = render_swim_frame(&snap, &coat, CameraView::Side, 24);
    let gif_bytes = encode_gif(&[swim_0, swim_1]).expect("encode_gif succeeds");
    assert!(gif_bytes.starts_with(b"GIF89a"));

    let card = render_share_card_frame(&snap, &coat, CameraView::Side);
    let card_png = encode_share_card_png(&card).expect("encode_share_card_png succeeds");
    assert!(card_png.starts_with(b"\x89PNG\r\n\x1a\n"));
}
