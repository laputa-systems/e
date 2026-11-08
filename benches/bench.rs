//! Wall-time and allocation benchmarks for real-world editor workflows.

#[cfg(target_os = "linux")]
use std::ffi::CStr;
use std::fs;
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use rustybench::{AllocProfiler, Bencher, black_box};

#[global_allocator]
static ALLOC: AllocProfiler = AllocProfiler::system();

use e::buffer::GapBuffer;
use e::command::CommandRegistry;
use e::command_buffer::{CommandBuffer, CommandBufferMode, CommandBufferResult};
use e::document::{Document, TextEdit};
use e::find::FindState;
use e::render::Renderer;
use e::selection::{CaretSet, Selection, TextPosition};
use e::viewport::Viewport;
use hi_lite::Language;

#[cfg(target_os = "linux")]
const TRACE_BEGIN: &CStr = c"BENCH_BEGIN";
#[cfg(target_os = "linux")]
const TRACE_END: &CStr = c"BENCH_END";

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn prctl(option: i32, ...) -> i32;
}

#[cfg(target_os = "linux")]
fn syscall_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("SYSCALL_TRACE").is_some())
}

#[cfg(target_os = "linux")]
fn trace_marker(marker: &CStr) {
    if syscall_trace_enabled() {
        unsafe {
            let _ = prctl(15, marker.as_ptr(), 0, 0, 0);
        }
    }
}

#[cfg(target_os = "linux")]
fn bench_with_syscall_trace<O>(bencher: Bencher, mut operation: impl FnMut() -> O) {
    bencher.bench_local(|| {
        trace_marker(TRACE_BEGIN);
        let result = operation();
        trace_marker(TRACE_END);
        black_box(result);
    });
}

#[cfg(not(target_os = "linux"))]
fn bench_with_syscall_trace<O>(bencher: Bencher, operation: impl FnMut() -> O) {
    bencher.bench_local(operation);
}

struct BenchDir(PathBuf);

impl BenchDir {
    fn new(label: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("e-bench-{label}-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for BenchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ── Realistic Fixtures ─────────────────────────────────────────────────────────

/// 4,700+ lines of genuine Rust source code from `e` itself.
const REAL_RUST_SOURCE: &str = concat!(
    include_str!("../src/editor.rs"),
    include_str!("../src/render.rs"),
    include_str!("../src/buffer.rs"),
);

/// Generate realistic Python code with varied function definitions, type hints,
/// docstrings, decorators, control flow, and nested collections.
fn append_realistic_python_block(buffer: &mut Vec<u8>, block_id: usize) {
    use std::io::Write;
    let _ = write!(
        buffer,
        r#"
@dataclass
class ServiceWorkerHandler_{block_id}:
    """Handles event dispatch and lifecycle management for worker instance {block_id}."""
    worker_id: str = "worker-{block_id:04x}"
    concurrency: int = 16
    metrics: dict[str, float] = field(default_factory=dict)

    def process_envelope(self, payload: dict[str, Any], priority: int = 0) -> bool:
        if priority < 0 or not payload.get("active", True):
            logger.warning("Dropping inactive or negative-priority envelope {block_id}")
            return False
        transformed = [
            (k, v * 1.5 if isinstance(v, (int, float)) else str(v))
            for k, v in payload.items()
            if not k.startswith("_")
        ]
        self.metrics["last_transformed_count"] = float(len(transformed))
        return len(transformed) > 0
"#
    );
}

fn make_realistic_python_bytes(target_bytes: usize) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(target_bytes);
    buffer.extend_from_slice(b"from __future__ import annotations\nimport logging\nfrom dataclasses import dataclass, field\nfrom typing import Any\n\nlogger = logging.getLogger(__name__)\n");
    let mut block = 0;
    while buffer.len() < target_bytes {
        append_realistic_python_block(&mut buffer, block);
        block += 1;
    }
    buffer.truncate(target_bytes);
    buffer
}

#[allow(clippy::too_many_arguments)]
fn render_editor_frame(
    renderer: &mut Renderer,
    buffer: &mut GapBuffer,
    viewport: &Viewport,
    cursor: TextPosition,
    selection: Option<Selection>,
    command_line: Option<&str>,
    find_matches: Option<&[(TextPosition, TextPosition)]>,
    find_current: Option<(TextPosition, TextPosition)>,
    find_active: bool,
    sink: &mut Vec<u8>,
) {
    sink.clear();
    renderer
        .render(
            sink,
            buffer,
            viewport,
            cursor.line,
            cursor.column,
            true,
            " main.rs",
            " e v0.1.13 ",
            command_line,
            selection,
            &[],
            &[],
            find_matches,
            find_current,
            &[],
            command_line.map(|line| line.len()),
            find_active,
            None,
        )
        .unwrap();
}

// ── 1. File I/O & Load Workflows ──────────────────────────────────────────────

#[rustybench::bench]
fn file_read_real_rust_source_160kb(bencher: Bencher) {
    let dir = BenchDir::new("read-rust");
    let path = dir.0.join("source.rs");
    fs::write(&path, REAL_RUST_SOURCE.as_bytes()).unwrap();
    bench_with_syscall_trace(bencher, || {
        black_box(e::file_io::read_file(&path).unwrap())
    });
}

#[rustybench::bench]
fn file_write_real_rust_source_160kb(bencher: Bencher) {
    let dir = BenchDir::new("write-rust");
    let path = dir.0.join("source.rs");
    let data = REAL_RUST_SOURCE.as_bytes();
    bench_with_syscall_trace(bencher, || {
        let cleaned = e::file_io::clean_for_write(data);
        e::file_io::write_file(&path, &cleaned, e::file_io::file_modification_time(&path))
            .unwrap();
        black_box(())
    });
}

#[rustybench::bench]
fn open_and_render_real_rust_source_4k_lines(bencher: Bencher) {
    let dir = BenchDir::new("open-rust");
    let path = dir.0.join("source.rs");
    fs::write(&path, REAL_RUST_SOURCE.as_bytes()).unwrap();
    let rules = Some(Language::Rust);

    bench_with_syscall_trace(bencher, || {
        let loaded = e::file_io::read_file(&path).unwrap();
        let mut buffer = GapBuffer::from_bytes(loaded);
        let mut renderer = Renderer::new();
        renderer.set_language(rules);
        let mut viewport = Viewport::new(120, 40);
        viewport.scroll_line = buffer.line_count() / 3;
        let cursor = TextPosition::new(viewport.scroll_line, 4);
        let mut sink = Vec::with_capacity(32 * 1024);
        render_editor_frame(
            &mut renderer,
            &mut buffer,
            &viewport,
            cursor,
            None,
            None,
            None,
            None,
            false,
            &mut sink,
        );
        black_box(sink.len())
    });
}

#[rustybench::bench]
fn open_and_render_python_1mb(bencher: Bencher) {
    let dir = BenchDir::new("open-1mb");
    let path = dir.0.join("service.py");
    let data = make_realistic_python_bytes(1024 * 1024);
    fs::write(&path, &data).unwrap();
    let rules = Some(Language::Python);

    bench_with_syscall_trace(bencher, || {
        let loaded = e::file_io::read_file(&path).unwrap();
        let mut buffer = GapBuffer::from_bytes(loaded);
        let mut renderer = Renderer::new();
        renderer.set_language(rules);
        let mut viewport = Viewport::new(120, 40);
        viewport.scroll_line = buffer.line_count() / 2;
        let cursor = TextPosition::new(viewport.scroll_line, 0);
        let mut sink = Vec::with_capacity(32 * 1024);
        render_editor_frame(
            &mut renderer,
            &mut buffer,
            &viewport,
            cursor,
            None,
            None,
            None,
            None,
            false,
            &mut sink,
        );
        black_box(sink.len())
    });
}

#[rustybench::bench]
fn open_and_render_python_10mb(bencher: Bencher) {
    let dir = BenchDir::new("open-10mb");
    let path = dir.0.join("large_service.py");
    let data = make_realistic_python_bytes(10 * 1024 * 1024);
    fs::write(&path, &data).unwrap();
    let rules = Some(Language::Python);

    bench_with_syscall_trace(bencher, || {
        let loaded = e::file_io::read_file(&path).unwrap();
        let mut buffer = GapBuffer::from_bytes(loaded);
        let mut renderer = Renderer::new();
        renderer.set_language(rules);
        let mut viewport = Viewport::new(120, 40);
        viewport.scroll_line = buffer.line_count() / 2;
        let cursor = TextPosition::new(viewport.scroll_line, 0);
        let mut sink = Vec::with_capacity(32 * 1024);
        render_editor_frame(
            &mut renderer,
            &mut buffer,
            &viewport,
            cursor,
            None,
            None,
            None,
            None,
            false,
            &mut sink,
        );
        black_box(sink.len())
    });
}

// ── 2. Interactive Editing Workflows ──────────────────────────────────────────

#[rustybench::bench]
fn interactive_typing_render_real_rust(b: Bencher) {
    let data = REAL_RUST_SOURCE.as_bytes().to_vec();
    let rules = Some(Language::Rust);
    let mut document = Document::new(data, Some("source.rs".to_string()));
    let mut renderer = Renderer::new();
    renderer.set_language(rules);
    let mut viewport = Viewport::new(120, 40);
    let cursor = TextPosition::new(1_200, 16);
    viewport.scroll_line = cursor.line;
    let mut sink = Vec::with_capacity(32 * 1024);

    // Warm up highlighter cache across the file down to viewport
    render_editor_frame(
        &mut renderer,
        &mut document.buffer,
        &viewport,
        cursor,
        None,
        None,
        None,
        None,
        false,
        &mut sink,
    );

    bench_with_syscall_trace(b, || {
        let after = document.insert(cursor.line, cursor.column, b"let x = 42; ");
        render_editor_frame(
            &mut renderer,
            &mut document.buffer,
            &viewport,
            after,
            None,
            None,
            None,
            None,
            false,
            &mut sink,
        );
        document.undo();
        black_box((after, sink.len()));
    });
}

#[rustybench::bench]
fn interactive_newline_indent_split_rust(b: Bencher) {
    let data = REAL_RUST_SOURCE.as_bytes().to_vec();
    let rules = Some(Language::Rust);
    let mut document = Document::new(data, Some("source.rs".to_string()));
    let mut renderer = Renderer::new();
    renderer.set_language(rules);
    let mut viewport = Viewport::new(120, 40);
    let cursor = TextPosition::new(1_200, 4);
    viewport.scroll_line = cursor.line;
    let mut sink = Vec::with_capacity(32 * 1024);

    render_editor_frame(
        &mut renderer,
        &mut document.buffer,
        &viewport,
        cursor,
        None,
        None,
        None,
        None,
        false,
        &mut sink,
    );

    bench_with_syscall_trace(b, || {
        let after = document.insert(cursor.line, cursor.column, b"\n        ");
        render_editor_frame(
            &mut renderer,
            &mut document.buffer,
            &viewport,
            after,
            None,
            None,
            None,
            None,
            false,
            &mut sink,
        );
        document.undo();
        black_box(after);
    });
}

#[rustybench::bench]
fn interactive_multicursor_typing_20_carets(b: Bencher) {
    let data = REAL_RUST_SOURCE.as_bytes().to_vec();
    let mut document = Document::new(data, Some("source.rs".to_string()));
    let mut carets = CaretSet::new(TextPosition::new(1_000, 4));
    for i in 1..20 {
        carets.add_caret(TextPosition::new(1_000 + i * 10, 4));
    }
    let before_snapshot = carets.snapshot();

    bench_with_syscall_trace(b, || {
        let edits: Vec<TextEdit> = before_snapshot
            .selections
            .iter()
            .map(|selection| {
                let offset = document
                    .buffer
                    .position_to_byte_offset(selection.cursor.line, selection.cursor.column);
                TextEdit {
                    start_byte: offset,
                    end_byte: offset,
                    inserted_bytes: b"// NOTE: ".to_vec(),
                    deleted_bytes: Vec::new(),
                }
            })
            .collect();
        let target_offsets: Vec<(usize, usize)> = edits
            .iter()
            .map(|e| (e.start_byte + 9, e.start_byte + 9))
            .collect();
        let after = document.apply_batch(&edits, &before_snapshot, &target_offsets, 0);
        document.undo();
        black_box(after);
    });
}

#[rustybench::bench]
fn paste_multiline_real_rust_100k(b: Bencher) {
    let mut document = Document::new(
        REAL_RUST_SOURCE.as_bytes().to_vec(),
        Some("source.rs".to_string()),
    );
    let paste_bytes = &REAL_RUST_SOURCE.as_bytes()[..100 * 1024];
    let cursor = TextPosition::new(2_000, 8);

    bench_with_syscall_trace(b, || {
        let after = document.insert(cursor.line, cursor.column, black_box(paste_bytes));
        document.undo();
        black_box(after);
    });
}

#[rustybench::bench]
fn document_batch_insert_and_full_undo(b: Bencher) {
    let data = REAL_RUST_SOURCE.as_bytes().to_vec();
    bench_with_syscall_trace(b, || {
        let mut document = Document::new(black_box(&data).clone(), None);
        for i in 0..10 {
            let line = 500 + i * 20;
            document.insert(line, 0, b"// audit header\n");
            document.seal_undo();
        }
        while document.undo().is_some() {}
        black_box(&document);
    });
}

// ── 3. Interactive Navigation & Viewport Rendering ────────────────────────────

#[rustybench::bench]
fn interactive_scroll_rust_continuous(b: Bencher) {
    let data = REAL_RUST_SOURCE.as_bytes().to_vec();
    let rules = Some(Language::Rust);
    let mut renderer = Renderer::new();
    renderer.set_language(rules);
    let mut buffer = GapBuffer::from_bytes(data);
    let mut viewport = Viewport::new(120, 40);
    viewport.scroll_line = 1_000;
    let mut sink = Vec::with_capacity(32 * 1024);

    // Initial warm frame
    render_editor_frame(
        &mut renderer,
        &mut buffer,
        &viewport,
        TextPosition::new(1_000, 0),
        None,
        None,
        None,
        None,
        false,
        &mut sink,
    );

    let mut down = true;
    bench_with_syscall_trace(b, || {
        if down {
            viewport.scroll_line += 3;
        } else {
            viewport.scroll_line -= 3;
        }
        down = !down;
        let cursor_line = viewport.scroll_line;
        render_editor_frame(
            &mut renderer,
            &mut buffer,
            &viewport,
            TextPosition::new(cursor_line, 0),
            None,
            None,
            None,
            None,
            false,
            &mut sink,
        );
        black_box(sink.len());
    });
}

#[rustybench::bench]
fn render_full_frame_rust_syntax(b: Bencher) {
    let data = REAL_RUST_SOURCE.as_bytes().to_vec();
    let rules = Some(Language::Rust);
    let mut renderer = Renderer::new();
    renderer.set_language(rules);
    let mut buffer = GapBuffer::from_bytes(data);
    let mut viewport = Viewport::new(120, 40);
    viewport.scroll_line = 1_500;
    let mut sink = Vec::with_capacity(32 * 1024);

    bench_with_syscall_trace(b, || {
        renderer.needs_full_redraw = true;
        render_editor_frame(
            &mut renderer,
            &mut buffer,
            &viewport,
            TextPosition::new(1_500, 4),
            None,
            None,
            None,
            None,
            false,
            &mut sink,
        );
        black_box(sink.len());
    });
}

#[rustybench::bench]
fn render_full_frame_rust_selection(b: Bencher) {
    let data = REAL_RUST_SOURCE.as_bytes().to_vec();
    let rules = Some(Language::Rust);
    let mut renderer = Renderer::new();
    renderer.set_language(rules);
    let mut buffer = GapBuffer::from_bytes(data);
    let mut viewport = Viewport::new(120, 40);
    viewport.scroll_line = 1_500;
    let selection = Selection {
        anchor: TextPosition::new(1_502, 0),
        cursor: TextPosition::new(1_518, 20),
    };
    let mut sink = Vec::with_capacity(32 * 1024);

    bench_with_syscall_trace(b, || {
        renderer.needs_full_redraw = true;
        render_editor_frame(
            &mut renderer,
            &mut buffer,
            &viewport,
            TextPosition::new(1_518, 20),
            Some(selection),
            None,
            None,
            None,
            false,
            &mut sink,
        );
        black_box(sink.len());
    });
}

#[rustybench::bench]
fn render_full_frame_plain(b: Bencher) {
    let data = REAL_RUST_SOURCE.as_bytes().to_vec();
    let mut renderer = Renderer::new();
    renderer.set_language(None);
    let mut buffer = GapBuffer::from_bytes(data);
    let mut viewport = Viewport::new(120, 40);
    viewport.scroll_line = 1_500;
    let mut sink = Vec::with_capacity(32 * 1024);

    bench_with_syscall_trace(b, || {
        renderer.needs_full_redraw = true;
        render_editor_frame(
            &mut renderer,
            &mut buffer,
            &viewport,
            TextPosition::new(1_500, 4),
            None,
            None,
            None,
            None,
            false,
            &mut sink,
        );
        black_box(sink.len());
    });
}

#[rustybench::bench]
fn render_incremental_cursor_move(b: Bencher) {
    let data = REAL_RUST_SOURCE.as_bytes().to_vec();
    let rules = Some(Language::Rust);
    let mut renderer = Renderer::new();
    renderer.set_language(rules);
    let mut buffer = GapBuffer::from_bytes(data);
    let viewport = Viewport::new(120, 40);
    let base_line = 1_500;
    let mut sink = Vec::with_capacity(32 * 1024);

    render_editor_frame(
        &mut renderer,
        &mut buffer,
        &viewport,
        TextPosition::new(base_line, 0),
        None,
        None,
        None,
        None,
        false,
        &mut sink,
    );

    let mut step = true;
    bench_with_syscall_trace(b, || {
        let line = if step { base_line + 1 } else { base_line };
        step = !step;
        render_editor_frame(
            &mut renderer,
            &mut buffer,
            &viewport,
            TextPosition::new(line, 4),
            None,
            None,
            None,
            None,
            false,
            &mut sink,
        );
        black_box(sink.len());
    });
}

// ── 4. Search & Replace Workflows ─────────────────────────────────────────────

#[rustybench::bench]
fn search_interactive_forward_real_rust(b: Bencher) {
    let buffer = GapBuffer::from_bytes(REAL_RUST_SOURCE.as_bytes().to_vec());
    let re = regex_lite::Regex::new(r"TextPosition").unwrap();
    let start = TextPosition::new(500, 0);

    bench_with_syscall_trace(b, || {
        black_box(FindState::search_forward(
            black_box(&buffer),
            &re,
            start,
        ))
    });
}

#[rustybench::bench]
fn search_regex_scan_real_rust(b: Bencher) {
    let buffer = GapBuffer::from_bytes(REAL_RUST_SOURCE.as_bytes().to_vec());
    let re = regex_lite::Regex::new(r"fn\s+[a-z_][a-z0-9_]*\s*\(").unwrap();

    bench_with_syscall_trace(b, || {
        black_box(FindState::search_forward(
            black_box(&buffer),
            &re,
            TextPosition::zero(),
        ))
    });
}

#[rustybench::bench]
fn find_command_palette_trace_real_rust(b: Bencher) {
    let mut document = Document::new(
        REAL_RUST_SOURCE.as_bytes().to_vec(),
        Some("source.rs".to_string()),
    );
    let mut renderer = Renderer::new();
    renderer.set_language(Some(Language::Rust));
    let viewport = Viewport::new(120, 40);
    let cursor = TextPosition::new(1_000, 4);
    let mut command = CommandBuffer::new();
    let mut find = FindState::new();
    let mut sink = Vec::with_capacity(32 * 1024);

    bench_with_syscall_trace(b, || {
        command.open(CommandBufferMode::Find, "find: ", "");
        find.clear();
        for ch in "TextPosition".chars() {
            let result = command.handle_key(e::input::Key::Char(ch));
            if let CommandBufferResult::Changed(pattern) = result {
                find.update_highlights_lazy(&pattern, &document.buffer, &viewport);
                let line = command.display_line();
                render_editor_frame(
                    &mut renderer,
                    &mut document.buffer,
                    &viewport,
                    cursor,
                    None,
                    Some(&line),
                    Some(&find.matches),
                    find.current,
                    false,
                    &mut sink,
                );
            }
        }
        find.update_highlights(command.input.as_str(), &document.buffer, &viewport);
        command.close();
        render_editor_frame(
            &mut renderer,
            &mut document.buffer,
            &viewport,
            cursor,
            None,
            None,
            Some(&find.matches),
            find.current,
            true,
            &mut sink,
        );
        black_box((&find, &sink));
    });
}

#[rustybench::bench]
fn replace_all_real_rust(b: Bencher) {
    let data = REAL_RUST_SOURCE.as_bytes().to_vec();
    let pattern = regex_lite::Regex::new(r"\bTextPosition\b").unwrap();

    bench_with_syscall_trace(b, || {
        let mut document = Document::new(black_box(&data).clone(), Some("source.rs".to_string()));
        let last_line = document.buffer.line_count().saturating_sub(1);
        let end = TextPosition::new(last_line, document.buffer.line_character_count(last_line));
        let text_bytes = document.text_in_range(TextPosition::zero(), end);
        let text = String::from_utf8_lossy(&text_bytes);
        let count = pattern.find_iter(&text).count();
        let replacement = pattern.replace_all(&text, "PositionCoord").into_owned();
        document.seal_undo();
        document.replace_range_with_deleted(
            TextPosition::zero(),
            end,
            replacement.as_bytes(),
            text_bytes,
        );
        document.seal_undo();
        black_box((count, document.buffer.len()));
    });
}

// ── 5. Command Palette Workflow ───────────────────────────────────────────────

#[rustybench::bench]
fn command_palette_goto_trace_real_rust(b: Bencher) {
    let data = REAL_RUST_SOURCE.as_bytes().to_vec();
    let mut document = Document::new(data, Some("source.rs".to_string()));
    let mut renderer = Renderer::new();
    renderer.set_language(Some(Language::Rust));
    let mut viewport = Viewport::new(120, 40);
    let mut cursor = TextPosition::new(200, 4);
    let mut command = CommandBuffer::new();
    let registry = CommandRegistry::new();
    let mut sink = Vec::with_capacity(32 * 1024);

    bench_with_syscall_trace(b, || {
        command.open(CommandBufferMode::Command, "> ", "");
        for ch in "goto 2500".chars() {
            let result = command.handle_key(e::input::Key::Char(ch));
            if matches!(result, CommandBufferResult::Changed(_)) {
                let line = command.display_line();
                render_editor_frame(
                    &mut renderer,
                    &mut document.buffer,
                    &viewport,
                    cursor,
                    None,
                    Some(&line),
                    None,
                    None,
                    false,
                    &mut sink,
                );
            }
        }
        let result = command.handle_key(e::input::Key::Char('\n'));
        if let CommandBufferResult::Submit(input) = result {
            if let e::command::CommandAction::Goto(line_1based) = registry.execute(&input) {
                let target_line = (line_1based.saturating_sub(1)).min(document.buffer.line_count() - 1);
                viewport.scroll_line = target_line.saturating_sub(15);
                cursor = TextPosition::new(target_line, 0);
            }
            command.close();
            render_editor_frame(
                &mut renderer,
                &mut document.buffer,
                &viewport,
                cursor,
                None,
                None,
                None,
                None,
                false,
                &mut sink,
            );
        }
        black_box((&command, &sink));
    });
}

fn main() {
    rustybench::main();
}
