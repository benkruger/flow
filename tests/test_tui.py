"""Tests for lib/tui.py — curses-based interactive TUI.

Tests mock curses.stdscr to exercise all TuiApp methods without a terminal.
"""

import curses
import json
import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from conftest import make_state, write_state

import tui


# --- Helpers ---


def _make_stdscr(rows=40, cols=80):
    """Create a mock curses stdscr object."""
    mock = MagicMock()
    mock.getmaxyx.return_value = (rows, cols)
    mock.getch.return_value = -1
    return mock


def _make_app(stdscr=None, root=None, flows=None, orch_data=None):
    """Create a TuiApp with mocked dependencies."""
    if stdscr is None:
        stdscr = _make_stdscr()
    app = tui.TuiApp(stdscr)
    if root is not None:
        app.root = Path(root)
    app.version = "0.36.2"
    if flows is not None:
        app.flows = flows
    if orch_data is not None:
        app.orch_data = orch_data
    return app


def _flow_from_state(state):
    """Build a flow summary dict from a state dict."""
    from tui_data import flow_summary
    return flow_summary(state)


# --- TuiApp initialization ---


def test_tui_app_init():
    """TuiApp initializes with default state."""
    stdscr = _make_stdscr()
    with patch("tui.project_root", return_value=Path("/tmp/test")):
        app = tui.TuiApp(stdscr)
    assert app.selected == 0
    assert app.view == "list"
    assert app.running is True
    assert app.confirming_abort is False


# --- _safe_addstr ---


def test_safe_addstr_normal():
    """Writes text within bounds."""
    stdscr = _make_stdscr()
    app = _make_app(stdscr)
    app._safe_addstr(0, 0, "hello")
    stdscr.addstr.assert_called_once_with(0, 0, "hello", 0)


def test_safe_addstr_truncates():
    """Truncates text when it exceeds available width."""
    stdscr = _make_stdscr(rows=10, cols=5)
    app = _make_app(stdscr)
    app._safe_addstr(0, 0, "hello world")
    stdscr.addstr.assert_called_once_with(0, 0, "hello", 0)


def test_safe_addstr_out_of_bounds_row():
    """Does not write when row is out of bounds."""
    stdscr = _make_stdscr(rows=10, cols=80)
    app = _make_app(stdscr)
    app._safe_addstr(10, 0, "hello")
    stdscr.addstr.assert_not_called()


def test_safe_addstr_negative_row():
    """Does not write when row is negative."""
    stdscr = _make_stdscr()
    app = _make_app(stdscr)
    app._safe_addstr(-1, 0, "hello")
    stdscr.addstr.assert_not_called()


def test_safe_addstr_col_past_end():
    """Does not write when col is past screen width."""
    stdscr = _make_stdscr(rows=10, cols=5)
    app = _make_app(stdscr)
    app._safe_addstr(0, 5, "hello")
    stdscr.addstr.assert_not_called()


def test_safe_addstr_with_attr():
    """Passes attributes through."""
    stdscr = _make_stdscr()
    app = _make_app(stdscr)
    app._safe_addstr(0, 0, "bold", curses.A_BOLD)
    stdscr.addstr.assert_called_once_with(0, 0, "bold", curses.A_BOLD)


def test_safe_addstr_curses_error():
    """Handles curses.error gracefully (e.g., writing to bottom-right corner)."""
    stdscr = _make_stdscr()
    stdscr.addstr.side_effect = curses.error("addstr() returned ERR")
    app = _make_app(stdscr)
    app._safe_addstr(0, 0, "hello")  # Should not raise


def test_safe_addstr_zero_available():
    """Does not write when available width is zero."""
    stdscr = _make_stdscr(rows=10, cols=5)
    app = _make_app(stdscr)
    app._safe_addstr(0, 5, "hello")
    stdscr.addstr.assert_not_called()


# --- _draw_list_view ---


def test_draw_list_view_empty():
    """Draws empty state message when no flows."""
    stdscr = _make_stdscr()
    app = _make_app(stdscr, flows=[])
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "No active flows" in text


def test_draw_list_view_with_flows():
    """Draws flow list and detail panel."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete",
                        "flow-code": "in_progress"},
    )
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "Test Feature" in text
    assert "Code" in text


def test_draw_list_view_multiple_flows_unselected_marker():
    """Non-selected flows get a plain marker (no arrow, no diamond)."""
    state1 = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete",
                        "flow-code": "in_progress"},
    )
    state2 = make_state(
        current_phase="flow-plan",
        phase_statuses={"flow-start": "complete", "flow-plan": "in_progress"},
    )
    state2["branch"] = "second-feature"
    flow1 = _flow_from_state(state1)
    flow2 = _flow_from_state(state2)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow1, flow2])
    app.selected = 0
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    # Selected flow gets arrow marker, second flow gets plain space marker
    assert "Second Feature" in text
    assert "\u25b8 " in text  # arrow for selected
    assert "  Second Feature" in text  # plain marker for unselected


def test_draw_list_view_with_notes_and_issues():
    """Shows notes and issues counts in detail panel."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete",
                        "flow-code": "in_progress"},
    )
    state["notes"] = [{"text": "a"}, {"text": "b"}]
    state["issues_filed"] = [{"url": "http://example.com"}]
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "Notes: 2" in text
    assert "Issues: 1" in text


def test_draw_list_view_with_issue_numbers():
    """Draws issue numbers in list view when prompt contains #N references."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete",
                        "flow-code": "in_progress"},
    )
    state["prompt"] = "work on #83 and #89"
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "#83" in text
    assert "#89" in text


def test_draw_list_view_no_issue_numbers():
    """No issue text appears when prompt has no #N references."""
    import re
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete",
                        "flow-code": "in_progress"},
    )
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    # Find the flow list row — contains both "Test Feature" and "Code" (phase info)
    flow_row_calls = [c for c in calls if "Test Feature" in c and "Code" in c]
    assert len(flow_row_calls) == 1
    flow_row_text = flow_row_calls[0]
    # PR #1 should appear, but no other #N pattern before it
    assert "PR #1" in flow_row_text
    # Remove "PR #1" and check no other #<digit> remains
    stripped = flow_row_text.replace("PR #1", "")
    assert not re.search(r"#\d", stripped)


def test_draw_list_view_no_pr():
    """Handles flow with no PR number."""
    state = make_state()
    state["pr_number"] = None
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    # Should not crash


def test_draw_list_view_small_terminal():
    """Handles very small terminal without crashing."""
    state = make_state()
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=10, cols=40)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()


# --- _draw_log_view ---


def test_draw_log_view_with_entries(state_dir):
    """Draws log entries from the log file."""
    branch = "test-feature"
    log_path = state_dir / f"{branch}.log"
    log_path.write_text("2026-01-01T10:15:00-08:00 Step 1 done\n")
    state = make_state()
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=20, cols=80)
    app = _make_app(stdscr, root=state_dir.parent, flows=[flow])
    app.view = "log"
    app._draw_log_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "10:15" in text
    assert "Step 1 done" in text


def test_draw_log_view_no_log_file(tmp_path):
    """Shows empty message when log file doesn't exist."""
    state = make_state()
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=20, cols=80)
    app = _make_app(stdscr, root=tmp_path, flows=[flow])
    app.view = "log"
    app._draw_log_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "No log entries" in text


def test_draw_log_view_no_flows():
    """Switches back to list view when no flows exist."""
    stdscr = _make_stdscr()
    app = _make_app(stdscr, flows=[])
    app.view = "log"
    app._draw_log_view()
    assert app.view == "list"


def test_draw_log_view_unreadable_log(state_dir):
    """Handles unreadable log file gracefully."""
    branch = "test-feature"
    log_dir = state_dir
    log_path = log_dir / f"{branch}.log"
    log_path.write_text("some content")
    log_path.chmod(0o000)
    state = make_state()
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=20, cols=80)
    app = _make_app(stdscr, root=state_dir.parent, flows=[flow])
    app.view = "log"
    app._draw_log_view()
    log_path.chmod(0o644)  # Restore for cleanup


# --- _handle_input ---


def test_handle_input_quit():
    """'q' key sets running to False."""
    app = _make_app()
    app._handle_input(ord("q"))
    assert app.running is False


def test_handle_input_escape_from_log():
    """Escape returns from log to list view."""
    app = _make_app()
    app.view = "log"
    app._handle_input(27)
    assert app.view == "list"


def test_handle_input_escape_in_list():
    """Escape does nothing in list view."""
    app = _make_app()
    app.view = "list"
    app._handle_input(27)
    assert app.view == "list"


# --- _handle_list_input ---


def test_navigate_up():
    """Arrow up decreases selected index."""
    state1 = make_state()
    state1["branch"] = "alpha"
    state2 = make_state()
    state2["branch"] = "bravo"
    app = _make_app(flows=[_flow_from_state(state1), _flow_from_state(state2)])
    app.selected = 1
    app._handle_list_input(curses.KEY_UP)
    assert app.selected == 0


def test_navigate_up_at_top():
    """Arrow up at top stays at 0."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    app.selected = 0
    app._handle_list_input(curses.KEY_UP)
    assert app.selected == 0


def test_navigate_down():
    """Arrow down increases selected index."""
    state1 = make_state()
    state1["branch"] = "alpha"
    state2 = make_state()
    state2["branch"] = "bravo"
    app = _make_app(flows=[_flow_from_state(state1), _flow_from_state(state2)])
    app.selected = 0
    app._handle_list_input(curses.KEY_DOWN)
    assert app.selected == 1


def test_navigate_down_at_bottom():
    """Arrow down at bottom stays at last index."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    app.selected = 0
    app._handle_list_input(curses.KEY_DOWN)
    assert app.selected == 0


def test_list_input_no_flows():
    """Input handling does nothing when no flows exist."""
    app = _make_app(flows=[])
    app._handle_list_input(curses.KEY_UP)
    assert app.selected == 0


def test_list_input_log_key():
    """'l' key switches to log view."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    app._handle_list_input(ord("l"))
    assert app.view == "log"


def test_list_input_refresh_key():
    """'r' key triggers refresh."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    with patch.object(app, "refresh_data") as mock_refresh:
        app._handle_list_input(ord("r"))
        mock_refresh.assert_called_once()


# --- _open_worktree ---


def test_open_worktree(tmp_path):
    """Opens worktree directory in Terminal."""
    worktree_dir = tmp_path / ".worktrees" / "test-feature"
    worktree_dir.mkdir(parents=True)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])
    with patch("tui.subprocess.Popen") as mock_popen:
        app._open_worktree()
        mock_popen.assert_called_once()
        args = mock_popen.call_args[0][0]
        assert args[0] == "open"
        assert args[1] == "-a"
        assert args[2] == "Terminal"


def test_open_worktree_no_dir(tmp_path):
    """Does nothing when worktree directory doesn't exist."""
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])
    with patch("tui.subprocess.Popen") as mock_popen:
        app._open_worktree()
        mock_popen.assert_not_called()


def test_open_worktree_no_flows():
    """Does nothing when no flows exist."""
    app = _make_app(flows=[])
    with patch("tui.subprocess.Popen") as mock_popen:
        app._open_worktree()
        mock_popen.assert_not_called()


# --- _open_pr ---


def test_open_pr():
    """Opens PR URL in browser."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    with patch("tui.subprocess.Popen") as mock_popen:
        app._open_pr()
        mock_popen.assert_called_once()
        args = mock_popen.call_args[0][0]
        assert args[0] == "open"
        assert "github.com" in args[1]


def test_open_pr_no_url():
    """Does nothing when PR URL is None."""
    state = make_state()
    state["pr_url"] = None
    app = _make_app(flows=[_flow_from_state(state)])
    with patch("tui.subprocess.Popen") as mock_popen:
        app._open_pr()
        mock_popen.assert_not_called()


def test_open_pr_no_flows():
    """Does nothing when no flows exist."""
    app = _make_app(flows=[])
    with patch("tui.subprocess.Popen") as mock_popen:
        app._open_pr()
        mock_popen.assert_not_called()


# --- abort flow ---


def test_start_abort():
    """'a' key starts abort confirmation."""
    state = make_state()
    stdscr = _make_stdscr()
    app = _make_app(stdscr, flows=[_flow_from_state(state)])
    app._start_abort()
    assert app.confirming_abort is True


def test_start_abort_no_flows():
    """Does nothing when no flows exist."""
    app = _make_app(flows=[])
    app._start_abort()
    assert app.confirming_abort is False


def test_abort_confirm_yes():
    """'y' confirms abort and calls _abort_flow."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    app.confirming_abort = True
    with patch.object(app, "_abort_flow") as mock_abort:
        app._handle_abort_confirm(ord("y"))
        mock_abort.assert_called_once()
    assert app.confirming_abort is False


def test_abort_confirm_capital_y():
    """'Y' also confirms abort."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    app.confirming_abort = True
    with patch.object(app, "_abort_flow") as mock_abort:
        app._handle_abort_confirm(ord("Y"))
        mock_abort.assert_called_once()


def test_abort_confirm_no():
    """Any other key cancels abort."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    app.confirming_abort = True
    with patch.object(app, "_abort_flow") as mock_abort:
        app._handle_abort_confirm(ord("n"))
        mock_abort.assert_not_called()
    assert app.confirming_abort is False


def test_handle_input_during_abort():
    """Input dispatches to abort confirm when confirming."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    app.confirming_abort = True
    with patch.object(app, "_handle_abort_confirm") as mock_confirm:
        app._handle_input(ord("n"))
        mock_confirm.assert_called_once_with(ord("n"))


def test_abort_flow_calls_cleanup():
    """_abort_flow calls bin/flow cleanup subprocess."""
    state = make_state()
    state["pr_number"] = 42
    app = _make_app(flows=[_flow_from_state(state)])
    with patch("tui.curses.endwin"), \
         patch("tui.curses.initscr") as mock_initscr, \
         patch("tui.curses.noecho"), \
         patch("tui.curses.cbreak"), \
         patch("tui.curses.curs_set"), \
         patch("tui.subprocess.run") as mock_run, \
         patch.object(app, "refresh_data"):
        mock_new_scr = _make_stdscr()
        mock_initscr.return_value = mock_new_scr
        app._abort_flow()
        mock_run.assert_called_once()
        cmd = mock_run.call_args[0][0]
        assert "cleanup" in cmd
        assert "--branch" in cmd
        assert "--pr" in cmd


def test_abort_flow_no_pr():
    """_abort_flow omits --pr when pr_number is None."""
    state = make_state()
    state["pr_number"] = None
    app = _make_app(flows=[_flow_from_state(state)])
    with patch("tui.curses.endwin"), \
         patch("tui.curses.initscr") as mock_initscr, \
         patch("tui.curses.noecho"), \
         patch("tui.curses.cbreak"), \
         patch("tui.curses.curs_set"), \
         patch("tui.subprocess.run") as mock_run, \
         patch.object(app, "refresh_data"):
        mock_new_scr = _make_stdscr()
        mock_initscr.return_value = mock_new_scr
        app._abort_flow()
        cmd = mock_run.call_args[0][0]
        assert "--pr" not in cmd


def test_abort_flow_no_flows():
    """_abort_flow does nothing when no flows exist."""
    app = _make_app(flows=[])
    with patch("tui.subprocess.run") as mock_run:
        app._abort_flow()
        mock_run.assert_not_called()


# --- refresh_data ---


def test_refresh_data(state_dir):
    """refresh_data loads flows from state files."""
    state = make_state()
    write_state(state_dir, "test-feature", state)
    app = _make_app(root=state_dir.parent)
    app.refresh_data()
    assert len(app.flows) == 1


def test_refresh_data_clamps_selected(state_dir):
    """refresh_data clamps selected when flows shrink."""
    state = make_state()
    write_state(state_dir, "test-feature", state)
    app = _make_app(root=state_dir.parent)
    app.selected = 5
    app.refresh_data()
    assert app.selected == 0


# --- run loop ---


def test_run_loop_quit():
    """Run loop exits on 'q' key."""
    stdscr = _make_stdscr()
    stdscr.getch.side_effect = [ord("q")]
    app = _make_app(stdscr, flows=[])
    with patch("tui.curses.curs_set"):
        app.run()
    assert app.running is False


def test_run_loop_refresh_on_timeout():
    """Run loop refreshes on getch timeout (-1)."""
    stdscr = _make_stdscr()
    stdscr.getch.side_effect = [-1, ord("q")]
    app = _make_app(stdscr, flows=[])
    with patch("tui.curses.curs_set"), \
         patch.object(app, "refresh_data"):
        app.run()


def test_run_loop_resize():
    """Run loop handles KEY_RESIZE."""
    stdscr = _make_stdscr()
    stdscr.getch.side_effect = [curses.KEY_RESIZE, ord("q")]
    app = _make_app(stdscr, flows=[])
    with patch("tui.curses.curs_set"), \
         patch.object(app, "refresh_data"):
        app.run()


def test_run_loop_draws_log_view():
    """Run loop draws log view when view is 'log'."""
    stdscr = _make_stdscr()
    state = make_state()
    flow = _flow_from_state(state)
    stdscr.getch.side_effect = [ord("q")]
    app = _make_app(stdscr, flows=[flow])
    app.view = "log"
    with patch("tui.curses.curs_set"), \
         patch.object(app, "_draw_log_view") as mock_draw:
        app.run()
        mock_draw.assert_called()


# --- enter key ---


def test_enter_key_opens_worktree():
    """Enter key calls _open_worktree."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    with patch.object(app, "_open_worktree") as mock_open:
        app._handle_list_input(ord("\n"))
        mock_open.assert_called_once()


# --- 'p' key ---


def test_p_key_opens_pr():
    """'p' key calls _open_pr."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    with patch.object(app, "_open_pr") as mock_open:
        app._handle_list_input(ord("p"))
        mock_open.assert_called_once()


# --- 'a' key ---


def test_a_key_starts_abort():
    """'a' key calls _start_abort."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    with patch.object(app, "_start_abort") as mock_abort:
        app._handle_list_input(ord("a"))
        mock_abort.assert_called_once()


# --- main / _main ---


def test_main_calls_wrapper():
    """main() calls curses.wrapper."""
    with patch("tui.curses.wrapper") as mock_wrapper:
        tui.main()
        mock_wrapper.assert_called_once_with(tui._main)


def test_main_function_creates_app():
    """_main creates a TuiApp and calls run."""
    stdscr = _make_stdscr()
    stdscr.getch.side_effect = [ord("q")]
    with patch("tui.project_root", return_value=Path("/tmp/test")), \
         patch("tui.curses.curs_set"):
        tui._main(stdscr)


# --- _draw_detail_panel ---


def test_draw_detail_panel_code_in_progress():
    """Detail panel shows annotation for in-progress code phase."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete",
                        "flow-code": "in_progress"},
    )
    state["code_task"] = 3
    state["diff_stats"] = {"files_changed": 5, "insertions": 127, "deletions": 48}
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_detail_panel(10)
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "task 3" in text


def test_draw_detail_panel_no_notes_no_issues():
    """Detail panel omits notes/issues when counts are zero."""
    state = make_state()
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_detail_panel(10)
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "Notes:" not in text
    assert "Issues:" not in text


# --- edge case coverage ---


def test_safe_addstr_col_at_boundary():
    """Available width is exactly zero (col == max_x - 1 after guard)."""
    stdscr = _make_stdscr(rows=10, cols=10)
    app = _make_app(stdscr)
    # col=9 gives available=1 (writes 1 char); col=10 hits col>=max_x guard.
    # For available<=0 we need col after max_x check but where max_x-col<=0.
    # This can't happen since col<max_x means available>=1. Remove unreachable code.
    # Instead, test col=9 writes 1 char.
    app._safe_addstr(0, 9, "hello")
    stdscr.addstr.assert_called_once_with(0, 9, "h", 0)


def test_handle_input_dispatches_to_list_in_list_view():
    """Non-quit/non-escape keys in list view dispatch to _handle_list_input."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    app.view = "list"
    with patch.object(app, "_handle_list_input") as mock_list:
        app._handle_input(curses.KEY_UP)
        mock_list.assert_called_once_with(curses.KEY_UP)


# --- _get_repo ---


def test_get_repo_from_flows():
    """Returns repo from the first flow's state dict."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    assert app._get_repo() == "test/test"


def test_get_repo_fallback_detect_repo():
    """Falls back to detect_repo when no flows exist."""
    app = _make_app(flows=[])
    with patch("tui.detect_repo", return_value="owner/repo") as mock_detect:
        result = app._get_repo()
        assert result == "owner/repo"
        mock_detect.assert_called_once_with(cwd=str(app.root))


def test_get_repo_no_source():
    """Returns None when no flows and detect_repo fails."""
    app = _make_app(flows=[])
    with patch("tui.detect_repo", return_value=None):
        assert app._get_repo() is None


def test_get_repo_flow_missing_repo():
    """Falls back to detect_repo when flow state has no repo key."""
    state = make_state()
    del state["repo"]
    app = _make_app(flows=[_flow_from_state(state)])
    with patch("tui.detect_repo", return_value="fallback/repo") as mock_detect:
        result = app._get_repo()
        assert result == "fallback/repo"
        mock_detect.assert_called_once()


# --- _open_issue ---


def test_open_issue_with_repo():
    """Opens issue URL in browser when repo is available."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    with patch("tui.subprocess.Popen") as mock_popen:
        app._open_issue(42)
        mock_popen.assert_called_once()
        args = mock_popen.call_args[0][0]
        assert args[0] == "open"
        assert args[1] == "https://github.com/test/test/issues/42"


def test_open_issue_no_repo():
    """Does nothing when repo is unavailable."""
    app = _make_app(flows=[])
    with patch("tui.detect_repo", return_value=None), \
         patch("tui.subprocess.Popen") as mock_popen:
        app._open_issue(42)
        mock_popen.assert_not_called()


def test_open_issue_no_flows_with_detect():
    """Opens issue URL via detect_repo fallback when no flows exist."""
    app = _make_app(flows=[])
    with patch("tui.detect_repo", return_value="owner/repo"), \
         patch("tui.subprocess.Popen") as mock_popen:
        app._open_issue(99)
        mock_popen.assert_called_once()
        args = mock_popen.call_args[0][0]
        assert args[1] == "https://github.com/owner/repo/issues/99"


def test_open_issue_with_explicit_repo():
    """Uses explicit repo parameter instead of _get_repo fallback."""
    app = _make_app(flows=[])
    with patch("tui.subprocess.Popen") as mock_popen:
        app._open_issue(42, repo="explicit/repo")
        mock_popen.assert_called_once()
        args = mock_popen.call_args[0][0]
        assert args[1] == "https://github.com/explicit/repo/issues/42"


# --- 'i' key ---


def test_i_key_opens_issue():
    """'i' key extracts issue number from prompt and opens it."""
    state = make_state()
    state["prompt"] = "fix issue #42"
    app = _make_app(flows=[_flow_from_state(state)])
    with patch.object(app, "_open_issue") as mock_open:
        app._handle_list_input(ord("i"))
        mock_open.assert_called_once_with(42, repo="test/test")


def test_i_key_no_issue_in_prompt():
    """'i' key does nothing when prompt has no issue reference."""
    state = make_state()
    state["prompt"] = "add new feature"
    app = _make_app(flows=[_flow_from_state(state)])
    with patch.object(app, "_open_issue") as mock_open:
        app._handle_list_input(ord("i"))
        mock_open.assert_not_called()


def test_open_flow_issue_no_flows():
    """_open_flow_issue does nothing when no flows exist."""
    app = _make_app(flows=[])
    with patch.object(app, "_open_issue") as mock_open:
        app._open_flow_issue()
        mock_open.assert_not_called()


def test_draw_list_view_footer_includes_issue():
    """Footer includes [i] Issue hint."""
    state = make_state()
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=120)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "[i] Issue" in text


# --- Tab bar and orchestration view ---


def _make_orch_data(items=None, elapsed="4h 12m", completed_count=0,
                    failed_count=0, total=0, is_running=True):
    """Build a minimal orchestration summary dict for tests."""
    return {
        "elapsed": elapsed,
        "completed_count": completed_count,
        "failed_count": failed_count,
        "total": total if total else len(items or []),
        "is_running": is_running,
        "items": items or [],
    }


def _make_orch_item(issue_number, title, icon="\u00b7", status="pending",
                    elapsed="", pr_url=None, reason=None):
    """Build a minimal orchestration queue item dict for tests."""
    return {
        "icon": icon,
        "issue_number": issue_number,
        "title": title,
        "elapsed": elapsed,
        "pr_url": pr_url,
        "reason": reason,
        "status": status,
    }


def test_tui_app_init_has_tab_state():
    """TuiApp initializes with tab-related state."""
    stdscr = _make_stdscr()
    with patch("tui.project_root", return_value=Path("/tmp/test")):
        app = tui.TuiApp(stdscr)
    assert app.active_tab == 0
    assert app.orch_data is None
    assert app.orch_selected == 0


def test_draw_list_view_shows_tab_bar():
    """Tab bar text appears in the list view output."""
    state = make_state()
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "Active Flows" in text


def test_tab_switch_right():
    """Right arrow moves to orchestration tab."""
    app = _make_app(flows=[])
    app.active_tab = 0
    app._handle_input(curses.KEY_RIGHT)
    assert app.active_tab == 1


def test_tab_switch_left():
    """Left arrow returns to flows tab."""
    app = _make_app(flows=[])
    app.active_tab = 1
    app._handle_input(curses.KEY_LEFT)
    assert app.active_tab == 0


def test_tab_switch_right_at_max():
    """Right arrow at tab 1 stays at 1."""
    app = _make_app(flows=[])
    app.active_tab = 1
    app._handle_input(curses.KEY_RIGHT)
    assert app.active_tab == 1


def test_tab_switch_left_at_min():
    """Left arrow at tab 0 stays at 0."""
    app = _make_app(flows=[])
    app.active_tab = 0
    app._handle_input(curses.KEY_LEFT)
    assert app.active_tab == 0


def test_draw_orchestration_view_no_state():
    """Shows 'No orchestration running' when orch_data is None."""
    stdscr = _make_stdscr(rows=20, cols=80)
    app = _make_app(stdscr, flows=[])
    app.active_tab = 1
    app._draw_orchestration_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "No orchestration running" in text


def test_draw_orchestration_view_with_queue():
    """Shows queue items with status icons."""
    items = [
        _make_orch_item(42, "Add PDF export", icon="\u2713",
                        status="completed", elapsed="1h 24m"),
        _make_orch_item(43, "Fix login", icon="\u2717",
                        status="failed", elapsed="1h 2m"),
        _make_orch_item(45, "Update hooks", icon="\u25b6",
                        status="in_progress", elapsed="38m"),
        _make_orch_item(46, "Add rate limiting", icon="\u00b7"),
    ]
    orch = _make_orch_data(items=items, completed_count=1, failed_count=1,
                           is_running=True)
    stdscr = _make_stdscr(rows=30, cols=80)
    app = _make_app(stdscr, flows=[], orch_data=orch)
    app.active_tab = 1
    app._draw_orchestration_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "\u2713" in text
    assert "\u2717" in text
    assert "\u25b6" in text
    assert "#42" in text
    assert "Add PDF export" in text


def test_draw_orchestration_view_shows_elapsed():
    """Shows total elapsed time."""
    orch = _make_orch_data(items=[], elapsed="4h 12m", is_running=True)
    stdscr = _make_stdscr(rows=20, cols=80)
    app = _make_app(stdscr, flows=[], orch_data=orch)
    app.active_tab = 1
    app._draw_orchestration_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "4h 12m" in text


def test_flow_list_cross_tab_indicator():
    """Shows diamond indicator on flow matching in-progress orchestration issue."""
    state1 = make_state()
    state1["branch"] = "alpha-feature"
    state1["prompt"] = "unrelated work"
    flow1 = _flow_from_state(state1)
    state2 = make_state()
    state2["branch"] = "bravo-feature"
    state2["prompt"] = "work on issue #42"
    flow2 = _flow_from_state(state2)
    items = [
        _make_orch_item(42, "Add PDF export", icon="\u25b6",
                        status="in_progress", elapsed="38m"),
    ]
    orch = _make_orch_data(items=items, is_running=True)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow1, flow2], orch_data=orch)
    app.selected = 0
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "\u25c6" in text


def test_orchestration_tab_up_down_navigation():
    """Up/down keys navigate the orchestration queue."""
    items = [
        _make_orch_item(42, "A"),
        _make_orch_item(43, "B"),
        _make_orch_item(44, "C"),
    ]
    orch = _make_orch_data(items=items)
    app = _make_app(flows=[], orch_data=orch)
    app.active_tab = 1
    app.orch_selected = 0
    app._handle_orch_input(curses.KEY_DOWN)
    assert app.orch_selected == 1
    app._handle_orch_input(curses.KEY_UP)
    assert app.orch_selected == 0
    app._handle_orch_input(curses.KEY_UP)
    assert app.orch_selected == 0


def test_run_loop_draws_orchestration_view():
    """Run loop draws orchestration view when active_tab is 1."""
    stdscr = _make_stdscr()
    stdscr.getch.side_effect = [ord("q")]
    app = _make_app(stdscr, flows=[])
    app.active_tab = 1
    with patch("tui.curses.curs_set"), \
         patch.object(app, "_draw_orchestration_view") as mock_draw:
        app.run()
        mock_draw.assert_called()


def test_orchestration_tab_count_in_tab_bar():
    """Tab bar shows Orchestration (N/M) when running."""
    items = [
        _make_orch_item(42, "A", icon="\u2713", status="completed"),
        _make_orch_item(43, "B", icon="\u2717", status="failed"),
        _make_orch_item(44, "C"),
    ]
    orch = _make_orch_data(items=items, completed_count=1, failed_count=1,
                           is_running=True)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[], orch_data=orch)
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "2/3" in text


def test_handle_input_dispatches_to_orch_in_orch_tab():
    """Input dispatches to _handle_orch_input when on orchestration tab."""
    app = _make_app(flows=[])
    app.active_tab = 1
    app.view = "list"
    with patch.object(app, "_handle_orch_input") as mock_orch:
        app._handle_input(curses.KEY_DOWN)
        mock_orch.assert_called_once_with(curses.KEY_DOWN)


# --- Orchestration view detail panel and keyboard ---


def test_orch_i_key_opens_issue():
    """'i' key opens issue URL in browser."""
    state = make_state()
    state["repo"] = "test/repo"
    flow = _flow_from_state(state)
    items = [_make_orch_item(42, "Add PDF export")]
    orch = _make_orch_data(items=items)
    app = _make_app(flows=[flow], orch_data=orch)
    app.active_tab = 1
    app.orch_selected = 0
    with patch("tui.subprocess.Popen") as mock_popen:
        app._handle_orch_input(ord("i"))
        mock_popen.assert_called_once()
        args = mock_popen.call_args[0][0]
        assert args[0] == "open"
        assert "test/repo" in args[1]
        assert "/issues/42" in args[1]


def test_orch_i_key_no_flows_uses_detect_repo():
    """'i' key falls back to detect_repo when no flows exist."""
    items = [_make_orch_item(42, "Add PDF export")]
    orch = _make_orch_data(items=items)
    app = _make_app(flows=[], orch_data=orch)
    app.active_tab = 1
    with patch("tui.detect_repo", return_value=None), \
         patch("tui.subprocess.Popen") as mock_popen:
        app._handle_orch_input(ord("i"))
        mock_popen.assert_not_called()


def test_orch_i_key_no_items():
    """'i' key does nothing when orch_data has no items."""
    orch = _make_orch_data(items=[])
    app = _make_app(flows=[], orch_data=orch)
    app.active_tab = 1
    with patch("tui.subprocess.Popen") as mock_popen:
        app._handle_orch_input(ord("i"))
        mock_popen.assert_not_called()


def test_orch_r_key_refreshes():
    """'r' key triggers refresh in orchestration tab."""
    items = [_make_orch_item(42, "A")]
    orch = _make_orch_data(items=items)
    app = _make_app(flows=[], orch_data=orch)
    app.active_tab = 1
    with patch.object(app, "refresh_data") as mock_refresh:
        app._handle_orch_input(ord("r"))
        mock_refresh.assert_called_once()


def test_orch_r_key_refreshes_no_items():
    """'r' key triggers refresh even with no orch items."""
    orch = _make_orch_data(items=[])
    app = _make_app(flows=[], orch_data=orch)
    app.active_tab = 1
    with patch.object(app, "refresh_data") as mock_refresh:
        app._handle_orch_input(ord("r"))
        mock_refresh.assert_called_once()


def test_orch_detail_panel_failed_shows_reason():
    """Detail panel shows failure reason for failed items."""
    items = [
        _make_orch_item(43, "Fix login", icon="\u2717", status="failed",
                        reason="CI failed after 3 attempts"),
    ]
    orch = _make_orch_data(items=items, failed_count=1)
    stdscr = _make_stdscr(rows=30, cols=80)
    app = _make_app(stdscr, flows=[], orch_data=orch)
    app.active_tab = 1
    app.orch_selected = 0
    app._draw_orchestration_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "Reason:" in text
    assert "CI failed after 3 attempts" in text


def test_orch_detail_panel_completed_shows_pr():
    """Detail panel shows PR URL for completed items."""
    items = [
        _make_orch_item(42, "Add PDF export", icon="\u2713", status="completed",
                        elapsed="1h 24m",
                        pr_url="https://github.com/test/test/pull/58"),
    ]
    orch = _make_orch_data(items=items, completed_count=1)
    stdscr = _make_stdscr(rows=30, cols=80)
    app = _make_app(stdscr, flows=[], orch_data=orch)
    app.active_tab = 1
    app.orch_selected = 0
    app._draw_orchestration_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "PR:" in text
    assert "pull/58" in text


def test_orch_view_item_with_pr_url():
    """Queue item line includes PR number when pr_url is set."""
    items = [
        _make_orch_item(42, "Done", icon="\u2713", status="completed",
                        elapsed="1h 24m",
                        pr_url="https://github.com/test/test/pull/58"),
    ]
    orch = _make_orch_data(items=items, completed_count=1)
    stdscr = _make_stdscr(rows=30, cols=80)
    app = _make_app(stdscr, flows=[], orch_data=orch)
    app.active_tab = 1
    app._draw_orchestration_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "PR 58" in text


def test_refresh_data_clamps_orch_selected(state_dir):
    """refresh_data clamps orch_selected when items shrink."""
    app = _make_app(root=state_dir.parent)
    app.orch_selected = 5
    # Write a valid orchestrate.json with 1 item
    orch = {
        "started_at": "2026-03-20T22:00:00-07:00",
        "completed_at": None,
        "queue": [{"issue_number": 42, "title": "A", "status": "pending",
                    "started_at": None, "completed_at": None, "outcome": None,
                    "pr_url": None, "branch": None, "reason": None}],
        "current_index": None,
    }
    (state_dir / "orchestrate.json").write_text(json.dumps(orch))
    app.refresh_data()
    assert app.orch_selected == 0


def test_get_orch_issue_in_progress_none_when_all_pending():
    """Returns None when no item is in_progress."""
    items = [_make_orch_item(42, "A"), _make_orch_item(43, "B")]
    orch = _make_orch_data(items=items)
    app = _make_app(flows=[], orch_data=orch)
    assert app._get_orch_issue_in_progress() is None


def test_draw_tab_bar_orch_not_running():
    """Tab bar shows 'Orchestration' without count when not running."""
    orch = _make_orch_data(items=[], is_running=False)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[], orch_data=orch)
    app._draw_tab_bar(2)
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "Orchestration" in text
    assert "/" not in text


def test_detail_panel_small_terminal():
    """Detail panel timeline breaks early on small terminal."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete",
                        "flow-code": "in_progress"},
    )
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=12, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_detail_panel(4)


def test_orch_input_no_data():
    """Orch input handler does nothing when orch_data is None."""
    app = _make_app(flows=[])
    app.active_tab = 1
    app.orch_data = None
    app._handle_orch_input(curses.KEY_DOWN)


def test_open_orch_issue_no_orch_data():
    """_open_orch_issue does nothing when orch_data is None."""
    app = _make_app(flows=[])
    app.orch_data = None
    with patch.object(app, "_open_issue") as mock_open:
        app._open_orch_issue()
        mock_open.assert_not_called()
