#!/usr/bin/env python3
"""Small, selective wrapper around the existing rustybench target."""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent.parent
TIERS = ('Edit', 'Change', 'Scenario', 'Full')
GROUPS = {
    'io': 'file_read_real_rust_source_160kb file_write_real_rust_source_160kb open_and_render_real_rust_source_4k_lines',
    'large': 'open_and_render_python_1mb open_and_render_python_10mb',
    'edit': 'interactive_typing_render_real_rust interactive_newline_indent_split_rust interactive_multicursor_typing_20_carets paste_multiline_real_rust_100k document_batch_insert_and_full_undo',
    'render': 'interactive_scroll_rust_continuous render_full_frame_rust_syntax render_full_frame_rust_selection render_full_frame_plain render_incremental_cursor_move',
    'search': 'search_interactive_forward_real_rust search_regex_scan_real_rust find_command_palette_trace_real_rust replace_all_real_rust',
    'command': 'command_palette_goto_trace_real_rust',
}
WORK = [('driver-self-test', 'Edit', 'harness')] + [
    (name, 'Change' if name != 'open_and_render_python_10mb' else 'Scenario', area)
    for area, names in GROUPS.items() for name in names.split()
]
# Full is reserved for the legacy unfiltered suite, explicitly requested only.
WORK.append(('all', 'Full', 'suite'))


def command(name):
    if name == 'driver-self-test':
        return [sys.executable, 'tests/bench_driver.py']
    cmd = ['cargo', 'bench', '--locked', '--bench', 'bench']
    return cmd if name == 'all' else cmd + ['--', 'bench::' + name, '--exact']


def select(args):
    names = {w[0] for w in WORK}
    areas = {w[2] for w in WORK}
    for values, valid, kind in ((args.name, names, 'name'), (args.area, areas, 'area'), (args.skip, names, 'skip')):
        unknown = set(values) - valid
        if unknown:
            raise ValueError('unknown ' + kind + ': ' + ', '.join(sorted(unknown)))
    ceiling = TIERS.index(args.tier)
    selected, excluded = [], []
    for w in WORK:
        name, tier, area = w
        reason = ('above tier ceiling' if TIERS.index(tier) > ceiling else
                  'not named' if args.name and name not in args.name else
                  'outside selected areas' if args.area and area not in args.area else
                  'skipped' if name in args.skip else None)
        # The unfiltered suite is an alternative, not an additional duplicate.
        if name == 'all' and args.tier == 'Full' and not args.name and not args.area:
            reason = 'select explicitly with --name all'
        (excluded if reason else selected).append((w, reason))
    if not selected:
        raise ValueError('selection matches no workloads')
    return [w for w, _ in selected], excluded


def preflight(selected):
    problems = []
    names = ', '.join(w[0] for w in selected)
    for tool in ('cargo', 'rustc') if any(w[1] != 'Edit' for w in selected) else ():
        if not shutil.which(tool):
            problems.append(f'{tool} missing; install the pinned Rust toolchain; needed by {names}')
        elif subprocess.run([tool, '--version'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
            problems.append(f'{tool} unavailable; repair the pinned Rust toolchain; needed by {names}')
    for path in ('Cargo.lock', 'Cargo.toml', 'benches/bench.rs', 'src/editor.rs', 'src/render.rs', 'src/buffer.rs') if any(w[1] != 'Edit' for w in selected) else ('tests/bench_driver.py',):
        if not (ROOT / path).is_file():
            problems.append(f'{path} missing; restore checkout; needed by {names}')
    if shutil.disk_usage(ROOT).free < 512 * 1024 * 1024:
        problems.append(f'less than 512 MiB free; free disk space; needed by {names}')
    return problems


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('action', choices=('run', 'check', 'list'))
    parser.add_argument('--tier', choices=TIERS, default='Edit')
    parser.add_argument('--name', action='append', default=[])
    parser.add_argument('--area', action='append', default=[])
    parser.add_argument('--skip', action='append', default=[])
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    if args.action == 'list':
        for name, tier, area in WORK:
            print(f'{name}\t{tier}\t{area}\t{ " ".join(command(name)) }')
        return 0
    try:
        selected, excluded = select(args)
    except ValueError as exc:
        print(exc, file=sys.stderr)
        return 2
    for (name, _, _), reason in excluded:
        print(f'excluded {name}: {reason}')
    problems = preflight(selected)
    if problems:
        for problem in problems:
            print('preflight: ' + problem, file=sys.stderr)
        return 1
    print(f'preflight ok; host={platform.platform()} load={os.getloadavg()} (informational)')
    print('Cargo rebuilds stale artifacts; fixtures are generated from this checkout.')
    for name, _, _ in selected:
        print(' '.join(command(name)))
    if args.action == 'check' or args.dry_run:
        return 0
    out = ROOT / '.work' / 'bench' / (datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True)
    def git(*args):
        return subprocess.run(['git', *args], cwd=ROOT, text=True, capture_output=True).stdout.strip()
    summary = {'commit': git('rev-parse', 'HEAD'), 'dirty': git('status', '--short'),
               'host': platform.platform(), 'load': os.getloadavg(), 'excluded': [dict(name=w[0], reason=r) for w, r in excluded],
               'timing_validity': 'No host gate or discarded-sample report; inconclusive for comparison.',
               'verdict': 'No budget; completion is not a performance verdict.', 'workloads': []}
    for name, tier, area in selected:
        start = time.monotonic()
        with (out / (name + '.log')).open('w') as log:
            result = subprocess.run(command(name), cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
        summary['workloads'].append(dict(name=name, tier=tier, area=area, exit=result.returncode, duration_seconds=round(time.monotonic()-start, 3)))
        print(f'{name}: exit {result.returncode}')
    (out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(f'summary: {out / "summary.json"}')
    return int(any(w['exit'] for w in summary['workloads']))


if __name__ == '__main__':
    sys.exit(main())
