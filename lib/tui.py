"""Interactive TUI for viewing and managing active FLOW features.

A curses-based terminal application that reads local state files and
provides keyboard-driven navigation. No Claude session required.

Usage: flow tui
"""

import curses
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from flow_utils import detect_repo, project_root, read_version, write_tab_sequences
from tui_data import (
    load_all_flows,
    load_orchestration,
    orchestration_summary,
    parse_log_entries,
    phase_timeline,
)

# Auto-refresh interval in milliseconds
REFRESH_MS = 2000

# Color pair IDs for curses.init_pair / curses.color_pair
COLOR_COMPLETE = 1
COLOR_ACTIVE = 2
COLOR_FAILED = 3
COLOR_HEADER = 4
COLOR_LINK = 5


class TuiApp:
    """Curses-based TUI application for FLOW."""

    def __init__(self, stdscr):
        self.stdscr = stdscr
        self.root = project_root()
        self.version = read_version()
        self.repo = detect_repo(cwd=str(self.root))
        self.repo_name = self.repo.split("/")[-1] if self.repo else None
        self.flows = []
        self.selected = 0
        self.view = "list"
        self.running = True
        self.confirming_abort = False
        self.active_tab = 0
        self.orch_data = None
        self.orch_selected = 0
        self.issue_selected = 0
        self.use_color = False

    def refresh_data(self):
        """Re-read all state files and orchestration state."""
        self.flows = load_all_flows(self.root)
        if self.selected >= len(self.flows):
            self.selected = max(0, len(self.flows) - 1)
        orch_state = load_orchestration(self.root)
        self.orch_data = orchestration_summary(orch_state)
        if self.orch_data and self.orch_selected >= len(self.orch_data["items"]):
            self.orch_selected = max(0, len(self.orch_data["items"]) - 1)

    def _init_colors(self):
        """Initialize color pairs if the terminal supports color."""
        if curses.has_colors():
            curses.start_color()
            curses.use_default_colors()
            curses.init_pair(COLOR_COMPLETE, curses.COLOR_GREEN, -1)
            curses.init_pair(COLOR_ACTIVE, curses.COLOR_YELLOW, -1)
            curses.init_pair(COLOR_FAILED, curses.COLOR_RED, -1)
            curses.init_pair(COLOR_HEADER, curses.COLOR_CYAN, -1)
            curses.init_pair(COLOR_LINK, curses.COLOR_BLUE, -1)
            self.use_color = True
        else:
            self.use_color = False

    def _color(self, pair_id):
        """Return the color pair attribute, or 0 if colors are unavailable."""
        if self.use_color:
            return curses.color_pair(pair_id)
        return 0

    def run(self):
        """Main loop."""
        curses.curs_set(0)
        self._init_colors()
        try:
            write_tab_sequences(repo=self.repo, root=str(self.root))
        except Exception:
            pass
        self.stdscr.timeout(REFRESH_MS)
        self.refresh_data()

        while self.running:
            self.stdscr.erase()
            if self.active_tab == 1:
                self._draw_orchestration_view()
            elif self.view == "list":
                self._draw_list_view()
            elif self.view == "log":
                self._draw_log_view()
            elif self.view == "issues":
                self._draw_issues_view()
            elif self.view == "tasks":
                self._draw_tasks_view()
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

    def _get_orch_issue_in_progress(self):
        """Return the issue_number of the in-progress orchestration item, or None."""
        if not self.orch_data:
            return None
        for item in self.orch_data["items"]:
            if item["status"] == "in_progress":
                return item["issue_number"]
        return None

    def _draw_tab_bar(self, row):
        """Draw the tab bar showing Active Flows and Orchestration tabs."""
        flows_label = f"Active Flows ({len(self.flows)})"
        if self.orch_data and self.orch_data["is_running"]:
            processed = self.orch_data["completed_count"] + self.orch_data["failed_count"]
            orch_label = f"Orchestration ({processed}/{self.orch_data['total']})"
        else:
            orch_label = "Orchestration"

        active_attr = curses.A_BOLD | self._color(COLOR_LINK)
        flows_attr = active_attr if self.active_tab == 0 else curses.A_DIM
        orch_attr = active_attr if self.active_tab == 1 else curses.A_DIM

        col = 2
        self._safe_addstr(row, col, flows_label, flows_attr)
        col += len(flows_label) + 2
        self._safe_addstr(row, col, "\u2502", curses.A_DIM)
        col += 2
        self._safe_addstr(row, col, orch_label, orch_attr)

    def _draw_header(self):
        """Draw the shared version header, repo name, tab bar, and separator."""
        _, max_x = self.stdscr.getmaxyx()
        border = "\u2500" * max_x
        self._safe_addstr(0, 0, border, curses.A_DIM)
        version_text = f" FLOW v{self.version} "
        self._safe_addstr(0, 2, version_text, self._color(COLOR_HEADER) | curses.A_BOLD)
        if self.repo_name:
            repo_col = 2 + len(version_text) + 1
            self._safe_addstr(0, repo_col, self.repo_name.upper(), self._color(COLOR_ACTIVE) | curses.A_BOLD)
        self._draw_tab_bar(2)
        self._safe_addstr(3, 2, "\u2500" * (max_x - 4), curses.A_DIM)

    def _draw_list_view(self):
        """Draw the flow list and detail panel."""
        max_y, max_x = self.stdscr.getmaxyx()

        self._draw_header()

        if not self.flows:
            self._safe_addstr(4, 2, "No active flows.")
            self._safe_addstr(6, 2, "Start a flow with: /flow:flow-start <feature>")
            self._safe_addstr(max_y - 1, 0, " [q] Quit", curses.A_DIM)
            return

        # Cross-tab indicator: find flow matching in-progress orchestration issue
        orch_issue = self._get_orch_issue_in_progress()

        # Flow list — reserve ~16 lines for header, separator, detail panel, and footer
        list_end = min(len(self.flows), max_y - 18)
        visible_flows = self.flows[:list_end]

        # Pre-compute column content for all visible flows to find max widths
        col_data = []
        for flow in visible_flows:
            annotation = flow["annotation"]
            phase_info = f"{flow['phase_number']}: {flow['phase_name']}"
            if annotation:
                phase_info += f" ({annotation})"
            pr_info = f"PR #{flow['pr_number']}" if flow["pr_number"] else ""
            issue_nums = flow.get("issue_numbers", set())
            issue_info = " ".join(f"#{n}" for n in sorted(issue_nums)) if issue_nums else ""
            elapsed_display = "Blocked" if flow["blocked"] else flow["elapsed"]
            col_data.append((phase_info, elapsed_display, issue_info, pr_info))

        # Dynamic column widths based on actual content (floors prevent collapse)
        phase_width = max((len(d[0]) for d in col_data), default=14)
        phase_width = max(phase_width, 14)
        issue_width = max((len(d[2]) for d in col_data), default=0)
        pr_width = max((len(d[3]) for d in col_data), default=0)

        # Responsive feature column: floor of 26, scales with terminal width
        # Overhead: 2 (col offset) + 2 (marker) + 3 (gap) + phase + 3 (gap) + 7 (elapsed) + 3 (gap) + 2 (right margin)
        overhead = 2 + 2 + 3 + phase_width + 3 + 7 + 3 + 2
        if issue_width:
            overhead += issue_width + 3
        if pr_width:
            overhead += pr_width + (2 if not issue_width else 0)
        feature_width = max(26, max_x - overhead)

        for i in range(list_end):
            flow = visible_flows[i]
            row = 4 + i
            if i == self.selected:
                marker = "\u25b8 "
            elif orch_issue and _flow_matches_issue(flow, orch_issue):
                marker = "\u25c6 "
            else:
                marker = "  "
            attr = curses.A_BOLD if i == self.selected else 0
            if flow["blocked"]:
                attr = attr | self._color(COLOR_FAILED)
            phase_info, elapsed_display, issue_info, pr_info = col_data[i]
            feature_display = flow["feature"]
            if len(feature_display) > feature_width:
                feature_display = feature_display[: feature_width - 3] + "..."
            feat = f"{feature_display:<{feature_width}s}"
            phase = f"{phase_info:<{phase_width}s}"
            elapsed = f"{elapsed_display:>7s}"
            parts = [marker, feat, "   ", phase, "   ", elapsed]
            if issue_width:
                parts.append("   ")
                parts.append(f"{issue_info:<{issue_width}s}")
            if pr_width:
                parts.append("  ")
                parts.append(f"{pr_info:<{pr_width}s}")
            line = "".join(parts)
            self._safe_addstr(row, 2, line, attr)

        # Separator
        detail_start = 4 + list_end + 1
        self._safe_addstr(detail_start - 1, 2, "\u2500" * (max_x - 4), curses.A_DIM)

        # Detail panel for selected flow
        if self.flows:
            self._draw_detail_panel(detail_start)

        # Footer
        footer = (
            " [\u2190\u2192] Tab  [\u2191\u2193] Navigate  [Enter] Worktree  [p] PR  [i] Issues"
            "  [I] Issue  [t] Tasks  [l] Log  [a] Abort  [r] Refresh  [q] Quit"
        )
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
        timeline = flow.get("timeline") or phase_timeline(state)
        for entry in timeline:
            if row >= max_y - 3:
                break
            if entry["status"] == "complete":
                marker = "[x]"
                suffix = f"  {entry['time']}" if entry["time"] else ""
                attr = self._color(COLOR_COMPLETE)
            elif entry["status"] == "in_progress":
                marker = "[>]"
                suffix = ""
                if entry["annotation"]:
                    suffix = f"  ({entry['annotation']})"
                if flow["blocked"]:
                    attr = self._color(COLOR_FAILED) | curses.A_BOLD
                else:
                    attr = self._color(COLOR_ACTIVE) | curses.A_BOLD
            else:
                marker = "[ ]"
                suffix = ""
                attr = curses.A_DIM
            line = f"{marker} {entry['name']}{suffix}"
            self._safe_addstr(row, 2, line, attr)
            row += 1

        row += 1
        if row < max_y - 2:
            parts = []
            if flow["notes_count"] > 0:
                parts.append(f"Notes: {flow['notes_count']}")
            if parts:
                self._safe_addstr(row, 2, "  \u2502  ".join(parts))
                row += 1
            for issue in flow.get("issues", []):
                if row >= max_y - 2:
                    break
                line = f"  {issue['ref']} {issue['title']}"
                self._safe_addstr(row, 2, line)
                row += 1

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

    def _draw_issues_view(self):
        """Draw the issues view for the selected flow."""
        max_y, max_x = self.stdscr.getmaxyx()

        if not self.flows:
            self.view = "list"
            return

        flow = self.flows[self.selected]
        issues = flow.get("issues", [])

        # Header
        header = f" {flow['feature']} \u2014 Issues "
        border = "\u2500" * max_x
        self._safe_addstr(0, 0, border, curses.A_DIM)
        self._safe_addstr(0, 2, header, curses.A_BOLD)

        if not issues:
            self._safe_addstr(2, 2, "No issues filed.")
        else:
            # Column header
            self._safe_addstr(2, 2, f"  {'Label':<18s} {'Ref':<8s} {'Phase':<14s} Title", curses.A_DIM)

            # Clamp selection
            if self.issue_selected >= len(issues):
                self.issue_selected = max(0, len(issues) - 1)

            for i, issue in enumerate(issues):
                if i >= max_y - 5:
                    break
                row = 3 + i
                marker = "\u25b8 " if i == self.issue_selected else "  "
                attr = curses.A_BOLD if i == self.issue_selected else 0
                label = issue["label"][:18]
                ref = issue["ref"]
                phase = issue["phase_name"][:14]
                title = issue["title"]
                line = f"{marker}{label:<18s} {ref:<8s} {phase:<14s} {title}"
                self._safe_addstr(row, 2, line, attr)

        # Footer
        footer = " [Esc] Back  [Enter] Open  [\u2191\u2193] Navigate  [q] Quit"
        self._safe_addstr(max_y - 1, 0, footer, curses.A_DIM)

    def _draw_tasks_view(self):
        """Draw the tasks/plan view for the selected flow."""
        max_y, max_x = self.stdscr.getmaxyx()

        if not self.flows:
            self.view = "list"
            return

        flow = self.flows[self.selected]
        plan_path = flow.get("plan_path")

        # Header
        header = f" {flow['feature']} \u2014 Tasks "
        border = "\u2500" * max_x
        self._safe_addstr(0, 0, border, curses.A_DIM)
        self._safe_addstr(0, 2, header, curses.A_BOLD)

        # Read plan file
        plan_content = None
        if plan_path:
            try:
                plan_content = Path(plan_path).read_text()
            except OSError:
                pass
            if plan_content is None:
                # Try relative to project root
                try:
                    plan_content = (self.root / plan_path).read_text()
                except OSError:
                    pass

        if plan_content is None:
            self._safe_addstr(2, 2, "No plan file.")
        else:
            lines = plan_content.split("\n")
            for i, line in enumerate(lines):
                row = 2 + i
                if row >= max_y - 2:
                    break
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
        elif key == curses.KEY_RIGHT:
            self.active_tab = min(1, self.active_tab + 1)
        elif key == curses.KEY_LEFT:
            self.active_tab = max(0, self.active_tab - 1)
        elif key == 27 and self.view in ("log", "issues", "tasks"):
            self.view = "list"
        elif self.active_tab == 1:
            self._handle_orch_input(key)
        elif self.view == "issues":
            self._handle_issues_input(key)
        elif self.view == "tasks":
            pass
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
        elif key == ord("i"):
            self.view = "issues"
        elif key == ord("t"):
            self.view = "tasks"
        elif key == ord("I"):
            self._open_flow_issue()
        elif key == ord("a"):
            self._start_abort()
        elif key == ord("r"):
            self.refresh_data()

    def _handle_issues_input(self, key):
        """Handle input in issues view."""
        if not self.flows:
            return
        flow = self.flows[self.selected]
        issues = flow.get("issues", [])
        if not issues:
            return

        if key == curses.KEY_UP:
            self.issue_selected = max(0, self.issue_selected - 1)
        elif key == curses.KEY_DOWN:
            self.issue_selected = min(len(issues) - 1, self.issue_selected + 1)
        elif key == ord("\n"):
            issue = issues[self.issue_selected]
            url = issue.get("url")
            if url:
                subprocess.Popen(
                    ["open", url],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )

    def _get_repo(self):
        """Get repo 'owner/repo' from flows or git remote fallback."""
        for flow in self.flows:
            repo = flow.get("state", {}).get("repo")
            if repo:
                return repo
        return detect_repo(cwd=str(self.root))

    def _open_flow_issue(self):
        """Open the GitHub issue referenced in the selected flow's prompt."""
        if not self.flows:
            return
        flow = self.flows[self.selected]
        issue_numbers = flow.get("issue_numbers", set())
        if issue_numbers:
            repo = flow["state"].get("repo")
            self._open_issue(min(issue_numbers), repo=repo)

    def _open_issue(self, issue_number, repo=None):
        """Open a GitHub issue by number in the browser."""
        if repo is None:
            repo = self._get_repo()
        if not repo:
            return
        url = f"https://github.com/{repo}/issues/{issue_number}"
        subprocess.Popen(
            ["open", url],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def _activate_iterm_tab(self, worktree_path):
        """Try to activate an existing iTerm2 tab whose CWD matches worktree_path.

        Uses AppleScript to iterate all iTerm2 windows/tabs and check each
        session's path variable (set by shell integration). Returns True if
        a matching tab was found and activated, False otherwise.
        """
        script = (
            'tell application "iTerm2"\n'
            "    repeat with w in windows\n"
            "        repeat with t in tabs of w\n"
            "            set s to current session of t\n"
            "            try\n"
            f'                if (variable named "path" of s) ends with "{worktree_path}" then\n'
            "                    select w\n"
            "                    select t\n"
            '                    return "true"\n'
            "                end if\n"
            "            end try\n"
            "        end repeat\n"
            "    end repeat\n"
            "end tell\n"
            'return "false"'
        )
        try:
            result = subprocess.run(
                ["osascript", "-e", script],
                capture_output=True,
                text=True,
                timeout=5,
            )
            return result.returncode == 0 and "true" in result.stdout.lower()
        except (subprocess.TimeoutExpired, OSError):
            return False

    def _open_worktree(self):
        """Open the selected flow's worktree in a terminal tab.

        For iTerm2, first tries to activate an existing tab whose CWD
        matches the worktree path. Falls back to opening a new tab.
        """
        if not self.flows:
            return
        flow = self.flows[self.selected]
        worktree_path = self.root / flow["worktree"]
        if worktree_path.is_dir():
            term = os.environ.get("TERM_PROGRAM", "")
            if term == "iTerm.app" and self._activate_iterm_tab(str(worktree_path)):
                return
            app = "iTerm" if term == "iTerm.app" else "Terminal"
            subprocess.Popen(
                ["open", "-a", app, str(worktree_path)],
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
                ["open", f"{pr_url.rstrip('/')}/files"],
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
            max_y - 1,
            0,
            f" Abort '{flow['feature']}'? [y/N] " + " " * 40,
            self._color(COLOR_FAILED) | curses.A_BOLD,
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
            str(bin_flow),
            "cleanup",
            str(self.root),
            "--branch",
            branch,
            "--worktree",
            worktree,
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
        self._init_colors()
        self.stdscr.timeout(REFRESH_MS)
        self.refresh_data()

    def _draw_orchestration_view(self):
        """Draw the orchestration queue view."""
        max_y, max_x = self.stdscr.getmaxyx()

        self._draw_header()

        if not self.orch_data:
            self._safe_addstr(5, 2, "No orchestration running.")
            self._safe_addstr(max_y - 1, 0, " [\u2190\u2192] Tab  [r] Refresh  [q] Quit", curses.A_DIM)
            return

        # Elapsed time
        self._safe_addstr(5, 2, f"Elapsed: {self.orch_data['elapsed']}")

        # Queue items
        items = self.orch_data["items"]
        list_start = 7
        list_end = min(len(items), max_y - 6)

        # Responsive orchestration title column: floor of 30, scales with terminal width
        # Overhead: 2 (col offset) + 2 (marker) + icon + " #NNN  " (~10) + 30 (elapsed/PR reserve) + 2 = 44
        orch_title_width = max(30, max_x - 44)

        for i in range(list_end):
            item = items[i]
            row = list_start + i
            marker = "\u25b8 " if i == self.orch_selected else "  "
            status = item["status"]
            if status == "completed":
                attr = self._color(COLOR_COMPLETE)
            elif status == "failed":
                attr = self._color(COLOR_FAILED)
            elif status == "in_progress":
                attr = self._color(COLOR_ACTIVE)
            else:
                attr = curses.A_DIM
            if i == self.orch_selected:
                attr = attr | curses.A_BOLD
            elapsed_str = f"  {item['elapsed']}" if item["elapsed"] else ""
            pr_str = ""
            if item["pr_url"]:
                pr_str = f"  PR {item['pr_url'].rstrip('/').rsplit('/', 1)[-1]}"
            title = f"{item['title']:<{orch_title_width}s}"
            line = f"{marker}{item['icon']} #{item['issue_number']}  {title}{elapsed_str}{pr_str}"
            self._safe_addstr(row, 2, line, attr)

        # Detail panel for selected item
        detail_row = list_start + list_end + 1
        if items and self.orch_selected < len(items):
            selected_item = items[self.orch_selected]
            if selected_item["status"] == "failed" and selected_item["reason"]:
                self._safe_addstr(detail_row, 4, f"Reason: {selected_item['reason']}")
            elif selected_item["status"] == "completed" and selected_item["pr_url"]:
                self._safe_addstr(detail_row, 4, f"PR: {selected_item['pr_url']}")

        # Footer
        footer = " [\u2190\u2192] Tab  [\u2191\u2193] Navigate  [i] Issue  [r] Refresh  [q] Quit"
        self._safe_addstr(max_y - 1, 0, footer, curses.A_DIM)

    def _handle_orch_input(self, key):
        """Handle input in orchestration tab."""
        if not self.orch_data or not self.orch_data["items"]:
            if key == ord("r"):
                self.refresh_data()
            return

        if key == curses.KEY_UP:
            self.orch_selected = max(0, self.orch_selected - 1)
        elif key == curses.KEY_DOWN:
            self.orch_selected = min(len(self.orch_data["items"]) - 1, self.orch_selected + 1)
        elif key == ord("i"):
            self._open_orch_issue()
        elif key == ord("r"):
            self.refresh_data()

    def _open_orch_issue(self):
        """Open the selected orchestration issue in a browser."""
        if not self.orch_data or not self.orch_data["items"]:
            return
        item = self.orch_data["items"][self.orch_selected]
        self._open_issue(item["issue_number"])


def _flow_matches_issue(flow, issue_number):
    """Check if a flow's prompt references the given issue number."""
    return issue_number in flow.get("issue_numbers", set())


def _main(stdscr):
    """Curses wrapper entry point."""
    app = TuiApp(stdscr)
    try:
        app.run()
    except KeyboardInterrupt:
        pass


def main():
    """Entry point for flow tui."""
    curses.wrapper(_main)


if __name__ == "__main__":
    main()
