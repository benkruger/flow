"""Interactive TUI for viewing and managing active FLOW features.

A curses-based terminal application that reads local state files and
provides keyboard-driven navigation. No Claude session required.

Usage: bin/flow tui
"""

import curses
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import project_root
from tui_data import load_all_flows, parse_log_entries, phase_timeline, read_version

# Auto-refresh interval in milliseconds
REFRESH_MS = 2000


class TuiApp:
    """Curses-based TUI application for FLOW."""

    def __init__(self, stdscr):
        self.stdscr = stdscr
        self.root = project_root()
        self.version = read_version()
        self.flows = []
        self.selected = 0
        self.view = "list"
        self.running = True
        self.confirming_abort = False

    def refresh_data(self):
        """Re-read all state files."""
        self.flows = load_all_flows(self.root)
        if self.selected >= len(self.flows):
            self.selected = max(0, len(self.flows) - 1)

    def run(self):
        """Main loop."""
        curses.curs_set(0)
        self.stdscr.timeout(REFRESH_MS)
        self.refresh_data()

        while self.running:
            self.stdscr.erase()
            if self.view == "list":
                self._draw_list_view()
            elif self.view == "log":
                self._draw_log_view()
            self.stdscr.refresh()

            key = self.stdscr.getch()
            if key == -1:
                self.refresh_data()
                continue
            if key == curses.KEY_RESIZE:
                self.refresh_data()
                continue
            self._handle_input(key)

    def _safe_addstr(self, row, col, text, attr=0):
        """Write text to screen, truncating to fit and ignoring overflow."""
        max_y, max_x = self.stdscr.getmaxyx()
        if row < 0 or row >= max_y or col >= max_x:
            return
        available = max_x - col
        truncated = text[:available]
        try:
            self.stdscr.addstr(row, col, truncated, attr)
        except curses.error:
            pass

    def _draw_list_view(self):
        """Draw the flow list and detail panel."""
        max_y, max_x = self.stdscr.getmaxyx()

        # Header
        header = f" FLOW v{self.version} "
        border = "\u2500" * max_x
        self._safe_addstr(0, 0, border, curses.A_DIM)
        self._safe_addstr(0, 2, header, curses.A_BOLD)

        if not self.flows:
            self._safe_addstr(2, 2, "No active flows.")
            self._safe_addstr(4, 2, "Start a flow with: /flow:flow-start <feature>")
            self._safe_addstr(max_y - 1, 0, " [q] Quit", curses.A_DIM)
            return

        # Flow list header
        count_label = f"Active Flows ({len(self.flows)})"
        self._safe_addstr(2, 2, count_label, curses.A_BOLD)
        self._safe_addstr(3, 2, "\u2500" * min(54, max_x - 4), curses.A_DIM)

        # Flow list — reserve ~16 lines for header, separator, detail panel, and footer
        list_end = min(len(self.flows), max_y - 16)
        for i in range(list_end):
            flow = self.flows[i]
            row = 4 + i
            marker = "\u25b8 " if i == self.selected else "  "
            attr = curses.A_BOLD if i == self.selected else 0
            phase_info = f"{flow['phase_number']}: {flow['phase_name']}"
            pr_info = f"PR #{flow['pr_number']}" if flow["pr_number"] else ""
            line = f"{marker}{flow['feature']:<26s} {phase_info:<14s} {flow['elapsed']:<8s} {pr_info}"
            self._safe_addstr(row, 2, line, attr)

        # Separator
        detail_start = 4 + list_end + 1
        self._safe_addstr(detail_start - 1, 2, "\u2500" * min(54, max_x - 4), curses.A_DIM)

        # Detail panel for selected flow
        if self.flows:
            self._draw_detail_panel(detail_start)

        # Footer
        footer = " [\u2191\u2193] Navigate  [Enter] Worktree  [p] PR  [l] Log  [a] Abort  [r] Refresh  [q] Quit"
        self._safe_addstr(max_y - 1, 0, footer, curses.A_DIM)

    def _draw_detail_panel(self, start_row):
        """Draw the detail panel for the selected flow."""
        flow = self.flows[self.selected]
        state = flow["state"]
        row = start_row

        self._safe_addstr(row, 2, flow["feature"], curses.A_BOLD)
        row += 1
        self._safe_addstr(row, 2, f"Branch: {flow['branch']}")
        row += 1
        self._safe_addstr(row, 2, f"Worktree: {flow['worktree']}")
        row += 2

        # Phase timeline
        max_y, _ = self.stdscr.getmaxyx()
        timeline = phase_timeline(state)
        for entry in timeline:
            if row >= max_y - 3:
                break
            if entry["status"] == "complete":
                marker = "[x]"
                suffix = f"  {entry['time']}" if entry["time"] else ""
            elif entry["status"] == "in_progress":
                marker = "[>]"
                suffix = ""
                if entry["annotation"]:
                    suffix = f"  ({entry['annotation']})"
            else:
                marker = "[ ]"
                suffix = ""
            line = f"{marker} {entry['name']}{suffix}"
            self._safe_addstr(row, 2, line)
            row += 1

        row += 1
        if row < max_y - 2:
            parts = []
            if flow["notes_count"] > 0:
                parts.append(f"Notes: {flow['notes_count']}")
            if flow["issues_count"] > 0:
                parts.append(f"Issues: {flow['issues_count']} filed")
            if parts:
                self._safe_addstr(row, 2, "  \u2502  ".join(parts))

    def _draw_log_view(self):
        """Draw the log view for the selected flow."""
        max_y, max_x = self.stdscr.getmaxyx()

        if not self.flows:
            self.view = "list"
            return

        flow = self.flows[self.selected]
        branch = flow["branch"]

        # Header
        header = f" {flow['feature']} \u2014 Log "
        border = "\u2500" * max_x
        self._safe_addstr(0, 0, border, curses.A_DIM)
        self._safe_addstr(0, 2, header, curses.A_BOLD)

        # Read log file
        log_path = self.root / ".flow-states" / f"{branch}.log"
        log_content = None
        if log_path.exists():
            try:
                log_content = log_path.read_text()
            except OSError:
                pass

        entries = parse_log_entries(log_content, limit=max_y - 4)

        if not entries:
            self._safe_addstr(2, 2, "No log entries.")
        else:
            for i, entry in enumerate(entries):
                row = 2 + i
                line = f"  {entry['time']}  {entry['message']}"
                self._safe_addstr(row, 2, line)

        # Footer
        footer = " [Esc] Back  [q] Quit"
        self._safe_addstr(max_y - 1, 0, footer, curses.A_DIM)

    def _handle_input(self, key):
        """Dispatch keyboard input."""
        if self.confirming_abort:
            self._handle_abort_confirm(key)
        elif key == ord("q"):
            self.running = False
        elif key == 27 and self.view == "log":
            self.view = "list"
        elif self.view == "list":
            self._handle_list_input(key)

    def _handle_list_input(self, key):
        """Handle input in list view."""
        if not self.flows:
            return

        if key == curses.KEY_UP:
            self.selected = max(0, self.selected - 1)
        elif key == curses.KEY_DOWN:
            self.selected = min(len(self.flows) - 1, self.selected + 1)
        elif key == ord("\n"):
            self._open_worktree()
        elif key == ord("p"):
            self._open_pr()
        elif key == ord("l"):
            self.view = "log"
        elif key == ord("a"):
            self._start_abort()
        elif key == ord("r"):
            self.refresh_data()

    def _open_worktree(self):
        """Open the selected flow's worktree in a new terminal tab."""
        if not self.flows:
            return
        flow = self.flows[self.selected]
        worktree_path = self.root / flow["worktree"]
        if worktree_path.is_dir():
            subprocess.Popen(
                ["open", "-a", "Terminal", str(worktree_path)],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )

    def _open_pr(self):
        """Open the selected flow's PR in a browser."""
        if not self.flows:
            return
        flow = self.flows[self.selected]
        pr_url = flow.get("pr_url")
        if pr_url:
            subprocess.Popen(
                ["open", str(pr_url)],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )

    def _start_abort(self):
        """Show abort confirmation prompt."""
        if not self.flows:
            return
        self.confirming_abort = True
        max_y, _ = self.stdscr.getmaxyx()
        flow = self.flows[self.selected]
        self._safe_addstr(
            max_y - 1, 0,
            f" Abort '{flow['feature']}'? [y/N] " + " " * 40,
            curses.A_BOLD,
        )
        self.stdscr.refresh()

    def _handle_abort_confirm(self, key):
        """Handle Y/N response to abort confirmation."""
        self.confirming_abort = False
        if key in (ord("y"), ord("Y")):
            self._abort_flow()

    def _abort_flow(self):
        """Abort the selected flow via bin/flow cleanup."""
        if not self.flows:
            return
        flow = self.flows[self.selected]
        branch = flow["branch"]
        worktree = flow["worktree"]
        pr_number = flow.get("pr_number")

        # Find bin/flow — use the plugin cache path or local bin/flow
        bin_flow = Path(__file__).resolve().parent.parent / "bin" / "flow"

        cmd = [
            str(bin_flow), "cleanup", str(self.root),
            "--branch", branch,
            "--worktree", worktree,
        ]
        if pr_number:
            cmd.extend(["--pr", str(pr_number)])

        curses.endwin()
        print(f"Aborting flow: {flow['feature']}...")
        subprocess.run(cmd)
        self.stdscr = curses.initscr()
        curses.noecho()
        curses.cbreak()
        self.stdscr.keypad(True)
        curses.curs_set(0)
        self.stdscr.timeout(REFRESH_MS)
        self.refresh_data()


def _main(stdscr):
    """Curses wrapper entry point."""
    app = TuiApp(stdscr)
    app.run()


def main():
    """Entry point for bin/flow tui."""
    curses.wrapper(_main)


if __name__ == "__main__":
    main()
