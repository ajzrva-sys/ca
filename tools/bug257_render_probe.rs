// Standalone optimized characterization of old prefix layout vs indexed rows.
// Uses the repository's ratatui rlib; not a whole-application benchmark.
use ratatui::{buffer::Buffer, layout::Rect, text::{Line, Text}, widgets::{Paragraph, Widget, Wrap}};
use std::{hint::black_box, time::Instant};

fn probe(groups: usize) {
    let mut lines = Vec::new();
    let mut headers = Vec::new();
    for group in 0..groups {
        headers.push(lines.len());
        lines.push(Line::from(format!("Edited group {group}")));
        for row in 0..15 {
            lines.push(Line::from(format!("History {group}/{row}: synthetic text with wrapping and ordinary terminal output. Additional words here.")));
        }
    }
    let area = Rect::new(0, 0, 100, 30);
    let mut before = Vec::new();
    let mut after = Vec::new();
    for _ in 0..3 {
        let start = Instant::now();
        for &index in &headers {
            if index != 0 {
                black_box(Paragraph::new(Text::from(lines[..index].to_vec())).wrap(Wrap { trim: false }).line_count(area.width));
            }
        }
        let paragraph = Paragraph::new(Text::from(lines.clone())).wrap(Wrap { trim: false });
        let rows = paragraph.line_count(area.width).min(u16::MAX as usize) as u16;
        let mut expected = Buffer::empty(area);
        paragraph.scroll((rows.saturating_sub(area.height), 0)).render(area, &mut expected);
        before.push(start.elapsed().as_secs_f64() * 1000.0);
        let start = Instant::now();
        let mut starts = vec![0u32];
        for line in &lines {
            let rows = Paragraph::new(line.clone()).wrap(Wrap { trim: false }).line_count(area.width) as u32;
            starts.push(starts.last().unwrap() + rows);
        }
        for &header in &headers { black_box(starts[header]); }
        let top = rows.saturating_sub(area.height);
        let end = u32::from(top) + u32::from(area.height);
        let begin = starts.partition_point(|row| *row <= u32::from(top)).saturating_sub(1);
        let mut actual = Buffer::empty(area);
        for index in begin..lines.len() {
            if starts[index] >= end { break; }
            let y = starts[index].saturating_sub(u32::from(top)) as u16;
            let skip = u32::from(top).saturating_sub(starts[index]) as u16;
            let height = starts[index + 1].min(end).saturating_sub(starts[index].max(u32::from(top))) as u16;
            Paragraph::new(lines[index].clone()).wrap(Wrap { trim: false }).scroll((skip, 0))
                .render(Rect::new(0, y, area.width, height), &mut actual);
        }
        after.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(actual, expected);
        black_box(actual);
    }
    before.sort_by(f64::total_cmp);
    after.sort_by(f64::total_cmp);
    println!("source_lines={} headers={groups} old_median_ms={:.2} indexed_median_ms={:.2}", lines.len(), before[1], after[1]);
}

fn main() { for groups in [32, 64, 128, 256] { probe(groups); } }
