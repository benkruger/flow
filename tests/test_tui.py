"""Tests for lib/tui.py — curses-based interactive TUI.

Tests mock curses.stdscr to exercise all TuiApp methods without a terminal.
"""

import curses
import json
import subprocess
from pathlib import Path
from unittest.mock import MagicMock, patch

import tui
from conftest import make_state, write_state

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
    app.repo = "test/test"
    app.repo_name = "test"
    app.use_color = False
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
    with patch("tui.project_root", return_value=Path("/tmp/test")), patch("tui.detect_repo", return_value=None):
        app = tui.TuiApp(stdscr)
    assert app.selected == 0
    assert app.view == "list"
    assert app.running is True
    assert app.confirming_abort is False


def test_tui_app_init_sets_repo_name():
    """TuiApp.__init__ sets repo_name from detect_repo."""
    stdscr = _make_stdscr()
    with (
        patch("tui.project_root", return_value=Path("/tmp/test")),
        patch("tui.detect_repo", return_value="owner/myrepo"),
    ):
        app = tui.TuiApp(stdscr)
    assert app.repo == "owner/myrepo"
    assert app.repo_name == "myrepo"


def test_tui_app_init_repo_name_none():
    """TuiApp.__init__ sets repo_name to None when detect_repo returns None."""
    stdscr = _make_stdscr()
    with patch("tui.project_root", return_value=Path("/tmp/test")), patch("tui.detect_repo", return_value=None):
        app = tui.TuiApp(stdscr)
    assert app.repo is None
    assert app.repo_name is None


def test_make_app_sets_use_color_false():
    """_make_app helper always sets use_color to False."""
    app = _make_app()
    assert app.use_color is False


# --- _draw_header ---


def test_draw_header_shows_repo_name():
    """Header renders repo name uppercased with yellow bold attribute."""
    stdscr = _make_stdscr(rows=10, cols=80)
    app = _make_app(stdscr)
    app.repo_name = "myrepo"
    app.use_color = True
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100):
        app._draw_header()
    calls = [c for c in stdscr.addstr.call_args_list]
    repo_calls = [c for c in calls if "MYREPO" in str(c[0][2])]
    assert len(repo_calls) >= 1
    # Should use COLOR_ACTIVE (yellow) | A_BOLD
    assert repo_calls[0][0][3] == tui.COLOR_ACTIVE * 100 | curses.A_BOLD


def test_draw_header_no_repo_name():
    """Header still renders version when repo_name is None."""
    stdscr = _make_stdscr(rows=10, cols=80)
    app = _make_app(stdscr)
    app.repo_name = None
    app._draw_header()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "FLOW" in text


def test_draw_header_separator_spans_terminal_width():
    """Header tab-bar separator spans max_x - 4 on wide terminals, not capped at 54."""
    stdscr = _make_stdscr(rows=10, cols=120)
    app = _make_app(stdscr)
    app._draw_header()
    # Row 3 is the tab-bar separator
    sep_calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 3]
    assert sep_calls, "Expected a separator call at row 3"
    sep_text = sep_calls[0][0][2]
    # Should be 116 chars (120 - 4), not capped at 54
    assert len(sep_text) == 116


def test_draw_list_view_separator_spans_terminal_width():
    """List/detail separator spans max_x - 4 on wide terminals, not capped at 54."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=120)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    # The list/detail separator is at row = 4 + list_end (1 flow) = row 5
    # It renders at detail_start - 1 = 5
    sep_calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 5 and "\u2500" in str(c[0][2])]
    assert sep_calls, "Expected a separator call at the list/detail boundary"
    sep_text = sep_calls[0][0][2]
    # Should be 116 chars (120 - 4), not capped at 54
    assert len(sep_text) == 116


# --- _init_colors ---


def test_init_colors_with_color_support():
    """_init_colors initializes color pairs when terminal supports color."""
    app = _make_app()
    with (
        patch("tui.curses.has_colors", return_value=True),
        patch("tui.curses.start_color") as mock_start,
        patch("tui.curses.use_default_colors") as mock_defaults,
        patch("tui.curses.init_pair") as mock_init_pair,
    ):
        app._init_colors()
        mock_start.assert_called_once()
        mock_defaults.assert_called_once()
        assert mock_init_pair.call_count == 5
        assert app.use_color is True


def test_init_colors_without_color_support():
    """_init_colors skips color setup when terminal has no color."""
    app = _make_app()
    with patch("tui.curses.has_colors", return_value=False), patch("tui.curses.init_pair") as mock_init_pair:
        app._init_colors()
        mock_init_pair.assert_not_called()
        assert app.use_color is False


# --- _color ---


def test_color_helper_with_color():
    """_color returns color_pair value when colors are enabled."""
    app = _make_app()
    app.use_color = True
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100) as mock_cp:
        result = app._color(tui.COLOR_COMPLETE)
        mock_cp.assert_called_once_with(tui.COLOR_COMPLETE)
        assert result == tui.COLOR_COMPLETE * 100


def test_color_helper_without_color():
    """_color returns 0 when colors are disabled."""
    app = _make_app()
    app.use_color = False
    assert app._color(tui.COLOR_COMPLETE) == 0


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
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
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
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
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
    """Shows notes count and per-issue lines in detail panel."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["notes"] = [{"text": "a"}, {"text": "b"}]
    state["issues_filed"] = [
        {
            "label": "Tech Debt",
            "title": "Fix date parser",
            "url": "https://github.com/test/test/issues/42",
            "phase": "flow-code",
            "phase_name": "Code",
            "timestamp": "2026-01-01T10:00:00-08:00",
        },
    ]
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "Notes: 2" in text
    assert "#42" in text
    assert "Fix date parser" in text


def test_draw_list_view_with_issue_numbers():
    """Draws issue numbers in list view when prompt contains #N references."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["prompt"] = "work on #83 and #89"
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=100)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "#83" in text
    assert "#89" in text


def test_draw_list_view_no_issue_numbers():
    """No issue text appears when prompt has no #N references."""
    import re
    from datetime import datetime

    from flow_utils import PACIFIC
    from tui_data import flow_summary

    now = datetime(2026, 1, 1, 0, 10, 0, tzinfo=PACIFIC)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    flow = flow_summary(state, now=now)
    stdscr = _make_stdscr(rows=40, cols=100)
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


def test_draw_list_view_feature_expands_on_wide_terminal():
    """Feature names are NOT truncated on wide terminals when they fit."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    # "Analyze Issues Blocked By Api" = 30 chars — fits at 120 cols
    state["branch"] = "analyze-issues-blocked-by-api"
    flow = _flow_from_state(state)
    full_name = flow["feature"]
    assert len(full_name) > 26, f"Test setup: need >26 chars, got {len(full_name)}"

    stdscr = _make_stdscr(rows=40, cols=120)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()

    # Find the list row (row 4 for the first flow)
    list_row_calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 4]
    assert list_row_calls, "Expected a call at row 4 for the flow list entry"
    list_row_text = list_row_calls[0][0][2]

    # On a wide terminal, the full name should appear without "..."
    assert full_name in list_row_text
    assert "..." not in list_row_text


def test_draw_list_view_feature_truncated_on_narrow_terminal():
    """Feature names are still truncated on narrow terminals."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    # "Showcase Slack Orchestrate Tui" = 30 chars — won't fit at 60 cols
    state["branch"] = "showcase-slack-orchestrate-tui"
    flow = _flow_from_state(state)
    full_name = flow["feature"]
    assert len(full_name) > 26, f"Test setup: need >26 chars, got {len(full_name)}"

    stdscr = _make_stdscr(rows=40, cols=60)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()

    # Find the list row (row 4 for the first flow)
    list_row_calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 4]
    assert list_row_calls, "Expected a call at row 4 for the flow list entry"
    list_row_text = list_row_calls[0][0][2]

    # On a narrow terminal, the name should be truncated
    assert "..." in list_row_text
    assert full_name not in list_row_text


def test_draw_list_view_long_feature_name_truncated():
    """Truncates feature names that exceed the responsive column width on narrow terminals."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    # Branch name that produces a feature name > 26 chars
    # "Showcase Slack Orchestrate Tui" = 30 chars
    state["branch"] = "showcase-slack-orchestrate-tui"
    flow = _flow_from_state(state)
    full_name = flow["feature"]
    assert len(full_name) > 26, f"Test setup: need >26 chars, got {len(full_name)}"

    # At 60 cols, feature_width = max(26, 60 - 49) = 26, so 30-char name is truncated
    stdscr = _make_stdscr(rows=40, cols=60)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()

    # Find the list row (row 4 for the first flow)
    list_row_calls = [
        c
        for c in stdscr.addstr.call_args_list
        if c[0][0] == 4  # row 4 = first flow entry
    ]
    assert list_row_calls, "Expected a call at row 4 for the flow list entry"
    list_row_text = list_row_calls[0][0][2]

    # List row should have truncated name with "..."
    assert "..." in list_row_text
    assert full_name not in list_row_text

    # Detail panel (rendered within _draw_list_view) should show the full name
    detail_calls = [
        c
        for c in stdscr.addstr.call_args_list
        if c[0][0] == 6  # detail panel first line (feature name bold)
    ]
    assert detail_calls, "Expected a detail panel call at row 6"
    detail_text = detail_calls[0][0][2]
    assert detail_text == full_name


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


# --- _activate_iterm_tab ---


def _osascript_result(stdout="", returncode=0):
    """Build a CompletedProcess for osascript calls."""
    return subprocess.CompletedProcess(args=[], returncode=returncode, stdout=stdout, stderr="")


def _ps_result(pids):
    """Build a CompletedProcess for ps calls returning PID list."""
    stdout = "\n".join(str(p) for p in pids) + "\n" if pids else ""
    return subprocess.CompletedProcess(args=[], returncode=0, stdout=stdout, stderr="")


def _lsof_result(cwd, returncode=0):
    """Build a CompletedProcess for lsof calls returning CWD."""
    stdout = f"p123\nfcwd\nn{cwd}\n" if returncode == 0 else ""
    return subprocess.CompletedProcess(args=[], returncode=returncode, stdout=stdout, stderr="")


def test_activate_iterm_tab_tty_cwd_match(tmp_path):
    """Returns True when tty-based CWD lookup finds a matching tab."""
    worktree_dir = tmp_path / ".worktrees" / "test-feature"
    worktree_dir.mkdir(parents=True)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])

    tty_output = "/dev/ttys001|1|1\n"
    calls = [
        _osascript_result(tty_output),  # collect ttys
        _ps_result([1234]),  # ps for ttys001
        _lsof_result(str(worktree_dir)),  # lsof for pid 1234
        _osascript_result("true"),  # activate tab
    ]
    with patch("tui.subprocess.run", side_effect=calls):
        assert app._activate_iterm_tab(str(worktree_dir)) is True


def test_activate_iterm_tab_no_match(tmp_path):
    """Returns False when no tty's CWD matches the worktree path."""
    worktree_dir = tmp_path / ".worktrees" / "test-feature"
    worktree_dir.mkdir(parents=True)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])

    tty_output = "/dev/ttys001|1|1\n"
    calls = [
        _osascript_result(tty_output),  # collect ttys
        _ps_result([1234]),  # ps for ttys001
        _lsof_result("/some/other/path"),  # lsof — no match
    ]
    with patch("tui.subprocess.run", side_effect=calls):
        assert app._activate_iterm_tab(str(worktree_dir)) is False


def test_activate_iterm_tab_lsof_permission_error(tmp_path):
    """Returns True when first PID fails lsof but second matches."""
    worktree_dir = tmp_path / ".worktrees" / "test-feature"
    worktree_dir.mkdir(parents=True)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])

    tty_output = "/dev/ttys001|1|1\n"
    calls = [
        _osascript_result(tty_output),  # collect ttys
        _ps_result([1111, 2222]),  # ps returns two PIDs
        _lsof_result("", returncode=1),  # lsof fails for pid 1111
        _lsof_result(str(worktree_dir)),  # lsof succeeds for pid 2222
        _osascript_result("true"),  # activate tab
    ]
    with patch("tui.subprocess.run", side_effect=calls):
        assert app._activate_iterm_tab(str(worktree_dir)) is True


def test_activate_iterm_tab_osascript_timeout(tmp_path):
    """Returns False when initial osascript (tty collection) times out."""
    worktree_dir = tmp_path / ".worktrees" / "test-feature"
    worktree_dir.mkdir(parents=True)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])
    with patch("tui.subprocess.run", side_effect=subprocess.TimeoutExpired(cmd="osascript", timeout=5)):
        assert app._activate_iterm_tab(str(worktree_dir)) is False


def test_activate_iterm_tab_osascript_error(tmp_path):
    """Returns False when initial osascript exits with non-zero status."""
    worktree_dir = tmp_path / ".worktrees" / "test-feature"
    worktree_dir.mkdir(parents=True)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])
    result = subprocess.CompletedProcess(args=[], returncode=1, stdout="", stderr="")
    with patch("tui.subprocess.run", return_value=result):
        assert app._activate_iterm_tab(str(worktree_dir)) is False


def test_activate_iterm_tab_no_sessions(tmp_path):
    """Returns False when osascript returns empty (no iTerm2 sessions)."""
    worktree_dir = tmp_path / ".worktrees" / "test-feature"
    worktree_dir.mkdir(parents=True)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])
    with patch("tui.subprocess.run", return_value=_osascript_result("")):
        assert app._activate_iterm_tab(str(worktree_dir)) is False


def test_activate_iterm_tab_activates_correct_tab(tmp_path):
    """The activation AppleScript targets the correct window and tab indices."""
    worktree_dir = tmp_path / ".worktrees" / "test-feature"
    worktree_dir.mkdir(parents=True)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])

    tty_output = "/dev/ttys001|2|3\n"
    calls = [
        _osascript_result(tty_output),  # collect ttys
        _ps_result([5678]),  # ps for ttys001
        _lsof_result(str(worktree_dir)),  # lsof matches
        _osascript_result("true"),  # activate tab
    ]
    with patch("tui.subprocess.run", side_effect=calls) as mock_run:
        app._activate_iterm_tab(str(worktree_dir))
        # The 4th call is the activation AppleScript
        activate_script = mock_run.call_args_list[3][0][0][2]
        assert "item 2 of windows" in activate_script
        assert "item 3 of tabs" in activate_script


def test_activate_iterm_tab_symlink_resolution(tmp_path):
    """Matches when worktree path is a symlink and CWD is the resolved path."""
    real_dir = tmp_path / "real-worktree"
    real_dir.mkdir()
    symlink_dir = tmp_path / ".worktrees" / "test-feature"
    symlink_dir.parent.mkdir(parents=True)
    symlink_dir.symlink_to(real_dir)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])

    tty_output = "/dev/ttys001|1|1\n"
    calls = [
        _osascript_result(tty_output),
        _ps_result([9999]),
        _lsof_result(str(real_dir)),  # lsof returns resolved path
        _osascript_result("true"),
    ]
    with patch("tui.subprocess.run", side_effect=calls):
        # Pass the symlink path — should still match via realpath
        assert app._activate_iterm_tab(str(symlink_dir)) is True


# --- _activate_iterm_tab tombstone ---


def test_activate_iterm_tab_no_shell_integration():
    """Tombstone: shell integration matching removed in PR #713. Must not return."""
    import inspect

    source = inspect.getsource(tui.TuiApp._activate_iterm_tab)
    assert 'variable named "path"' not in source


def test_activate_iterm_tab_malformed_line(tmp_path):
    """Skips lines that don't have exactly 3 pipe-separated fields."""
    worktree_dir = tmp_path / ".worktrees" / "test-feature"
    worktree_dir.mkdir(parents=True)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])

    # First line is malformed (only 2 fields), second matches
    tty_output = "/dev/ttys001|1\n/dev/ttys002|1|1\n"
    calls = [
        _osascript_result(tty_output),
        _ps_result([1234]),
        _lsof_result(str(worktree_dir)),
        _osascript_result("true"),
    ]
    with patch("tui.subprocess.run", side_effect=calls):
        assert app._activate_iterm_tab(str(worktree_dir)) is True


def test_activate_iterm_tab_ps_timeout(tmp_path):
    """Skips tty when ps times out and continues to next."""
    worktree_dir = tmp_path / ".worktrees" / "test-feature"
    worktree_dir.mkdir(parents=True)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])

    tty_output = "/dev/ttys001|1|1\n/dev/ttys002|1|2\n"
    calls = [
        _osascript_result(tty_output),
        subprocess.TimeoutExpired(cmd="ps", timeout=5),  # ps fails for ttys001
        _ps_result([5678]),  # ps works for ttys002
        _lsof_result(str(worktree_dir)),
        _osascript_result("true"),
    ]
    with patch("tui.subprocess.run", side_effect=calls):
        assert app._activate_iterm_tab(str(worktree_dir)) is True


def test_activate_iterm_tab_lsof_timeout(tmp_path):
    """Skips PID when lsof times out and continues to next."""
    worktree_dir = tmp_path / ".worktrees" / "test-feature"
    worktree_dir.mkdir(parents=True)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])

    tty_output = "/dev/ttys001|1|1\n"
    calls = [
        _osascript_result(tty_output),
        _ps_result([1111, 2222]),
        subprocess.TimeoutExpired(cmd="lsof", timeout=5),  # lsof fails for pid 1111
        _lsof_result(str(worktree_dir)),  # lsof succeeds for pid 2222
        _osascript_result("true"),
    ]
    with patch("tui.subprocess.run", side_effect=calls):
        assert app._activate_iterm_tab(str(worktree_dir)) is True


def test_activate_iterm_tab_activation_timeout(tmp_path):
    """Returns True even when activation osascript times out (best-effort)."""
    worktree_dir = tmp_path / ".worktrees" / "test-feature"
    worktree_dir.mkdir(parents=True)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])

    tty_output = "/dev/ttys001|1|1\n"
    calls = [
        _osascript_result(tty_output),
        _ps_result([1234]),
        _lsof_result(str(worktree_dir)),
        subprocess.TimeoutExpired(cmd="osascript", timeout=5),  # activation fails
    ]
    with patch("tui.subprocess.run", side_effect=calls):
        assert app._activate_iterm_tab(str(worktree_dir)) is True


# --- _open_worktree ---


def test_open_worktree_calls_activate(tmp_path):
    """Enter key delegates to _activate_iterm_tab with worktree path."""
    worktree_dir = tmp_path / ".worktrees" / "test-feature"
    worktree_dir.mkdir(parents=True)
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])
    with patch.object(app, "_activate_iterm_tab", return_value=True) as mock_activate:
        app._open_worktree()
        mock_activate.assert_called_once_with(str(worktree_dir))


def test_open_worktree_no_dir(tmp_path):
    """Does nothing when worktree directory doesn't exist."""
    state = make_state()
    app = _make_app(root=tmp_path, flows=[_flow_from_state(state)])
    with patch.object(app, "_activate_iterm_tab") as mock_activate:
        app._open_worktree()
        mock_activate.assert_not_called()


def test_open_worktree_no_flows():
    """Does nothing when no flows exist."""
    app = _make_app(flows=[])
    with patch.object(app, "_activate_iterm_tab") as mock_activate:
        app._open_worktree()
        mock_activate.assert_not_called()


# --- _open_worktree tombstone ---


def test_open_worktree_no_terminal_fallback():
    """Tombstone: open -a fallback removed in PR #713. Must not return."""
    import inspect

    source = inspect.getsource(tui.TuiApp._open_worktree)
    assert "open" not in source or "open -a" not in source
    assert "Terminal" not in source


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
        assert args[1].endswith("/files")


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


def test_abort_prompt_red_bold():
    """Abort confirmation prompt renders with red color pair OR'd with A_BOLD."""
    state = make_state()
    stdscr = _make_stdscr()
    app = _make_app(stdscr, flows=[_flow_from_state(state)])
    app.use_color = True
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100):
        app._start_abort()
    prompt_calls = [c for c in stdscr.addstr.call_args_list if "Abort" in str(c[0][2])]
    assert len(prompt_calls) == 1
    assert prompt_calls[0][0][3] == tui.COLOR_FAILED * 100 | curses.A_BOLD


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
    with (
        patch("tui.curses.endwin"),
        patch("tui.curses.initscr") as mock_initscr,
        patch("tui.curses.noecho"),
        patch("tui.curses.cbreak"),
        patch("tui.curses.curs_set"),
        patch.object(app, "_init_colors"),
        patch("tui.subprocess.run") as mock_run,
        patch.object(app, "refresh_data"),
    ):
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
    with (
        patch("tui.curses.endwin"),
        patch("tui.curses.initscr") as mock_initscr,
        patch("tui.curses.noecho"),
        patch("tui.curses.cbreak"),
        patch("tui.curses.curs_set"),
        patch.object(app, "_init_colors"),
        patch("tui.subprocess.run") as mock_run,
        patch.object(app, "refresh_data"),
    ):
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


def test_abort_flow_calls_init_colors():
    """_abort_flow re-initializes colors after curses re-init."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    with (
        patch("tui.curses.endwin"),
        patch("tui.curses.initscr") as mock_initscr,
        patch("tui.curses.noecho"),
        patch("tui.curses.cbreak"),
        patch("tui.curses.curs_set"),
        patch.object(app, "_init_colors") as mock_init_colors,
        patch("tui.subprocess.run"),
        patch.object(app, "refresh_data"),
    ):
        mock_new_scr = _make_stdscr()
        mock_initscr.return_value = mock_new_scr
        app._abort_flow()
        mock_init_colors.assert_called_once()


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


def test_run_calls_init_colors():
    """run() calls _init_colors before the main loop."""
    stdscr = _make_stdscr()
    stdscr.getch.side_effect = [ord("q")]
    app = _make_app(stdscr, flows=[])
    with patch("tui.curses.curs_set"), patch.object(app, "_init_colors") as mock_init:
        app.run()
        mock_init.assert_called_once()


def test_run_calls_write_tab_sequences():
    """run() calls write_tab_sequences on startup."""
    stdscr = _make_stdscr()
    stdscr.getch.side_effect = [ord("q")]
    app = _make_app(stdscr, flows=[])
    with patch("tui.curses.curs_set"), patch.object(app, "_init_colors"), patch("tui.write_tab_sequences") as mock_tabs:
        app.run()
        mock_tabs.assert_called_once_with(repo=app.repo, root=str(app.root))


def test_run_write_tab_sequences_failure_ignored():
    """run() continues if write_tab_sequences raises an error."""
    stdscr = _make_stdscr()
    stdscr.getch.side_effect = [ord("q")]
    app = _make_app(stdscr, flows=[])
    with (
        patch("tui.curses.curs_set"),
        patch.object(app, "_init_colors"),
        patch("tui.write_tab_sequences", side_effect=OSError("no tty")),
    ):
        app.run()
    assert app.running is False


def test_run_loop_quit():
    """Run loop exits on 'q' key."""
    stdscr = _make_stdscr()
    stdscr.getch.side_effect = [ord("q")]
    app = _make_app(stdscr, flows=[])
    with patch("tui.curses.curs_set"), patch.object(app, "_init_colors"):
        app.run()
    assert app.running is False


def test_run_loop_refresh_on_timeout():
    """Run loop refreshes on getch timeout (-1)."""
    stdscr = _make_stdscr()
    stdscr.getch.side_effect = [-1, ord("q")]
    app = _make_app(stdscr, flows=[])
    with patch("tui.curses.curs_set"), patch.object(app, "_init_colors"), patch.object(app, "refresh_data"):
        app.run()


def test_run_loop_resize():
    """Run loop handles KEY_RESIZE."""
    stdscr = _make_stdscr()
    stdscr.getch.side_effect = [curses.KEY_RESIZE, ord("q")]
    app = _make_app(stdscr, flows=[])
    with patch("tui.curses.curs_set"), patch.object(app, "_init_colors"), patch.object(app, "refresh_data"):
        app.run()


def test_run_loop_draws_log_view():
    """Run loop draws log view when view is 'log'."""
    stdscr = _make_stdscr()
    state = make_state()
    flow = _flow_from_state(state)
    stdscr.getch.side_effect = [ord("q")]
    app = _make_app(stdscr, flows=[flow])
    app.view = "log"
    with (
        patch("tui.curses.curs_set"),
        patch.object(app, "_init_colors"),
        patch.object(app, "_draw_log_view") as mock_draw,
    ):
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
    with (
        patch("tui.project_root", return_value=Path("/tmp/test")),
        patch("tui.curses.curs_set"),
        patch("tui.curses.has_colors", return_value=False),
    ):
        tui._main(stdscr)


def test_main_handles_keyboard_interrupt():
    """_main catches KeyboardInterrupt so Ctrl+C exits cleanly."""
    stdscr = _make_stdscr()
    stdscr.getch.side_effect = KeyboardInterrupt
    with (
        patch("tui.project_root", return_value=Path("/tmp/test")),
        patch("tui.curses.curs_set"),
        patch("tui.curses.has_colors", return_value=False),
    ):
        tui._main(stdscr)


# --- _draw_detail_panel ---


def test_detail_panel_complete_phase_green():
    """Phase timeline [x] markers render with green color pair."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app.use_color = True
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100):
        app._draw_detail_panel(10)
    complete_calls = [c for c in stdscr.addstr.call_args_list if "[x]" in str(c[0][2])]
    assert len(complete_calls) >= 1
    for call in complete_calls:
        assert call[0][3] == tui.COLOR_COMPLETE * 100


def test_detail_panel_in_progress_phase_yellow_bold():
    """Phase timeline [>] markers render with yellow color pair OR'd with A_BOLD."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app.use_color = True
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100):
        app._draw_detail_panel(10)
    in_progress_calls = [c for c in stdscr.addstr.call_args_list if "[>]" in str(c[0][2])]
    assert len(in_progress_calls) >= 1
    for call in in_progress_calls:
        assert call[0][3] == tui.COLOR_ACTIVE * 100 | curses.A_BOLD


def test_detail_panel_pending_phase_dim():
    """Phase timeline [ ] markers render with A_DIM."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_detail_panel(10)
    pending_calls = [c for c in stdscr.addstr.call_args_list if "[ ]" in str(c[0][2])]
    assert len(pending_calls) >= 1
    for call in pending_calls:
        assert call[0][3] == curses.A_DIM


def test_draw_detail_panel_code_in_progress():
    """Detail panel shows annotation for in-progress code phase."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 3
    state["diff_stats"] = {"files_changed": 5, "insertions": 127, "deletions": 48}
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_detail_panel(10)
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "task 4" in text


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


def test_draw_detail_panel_with_issues():
    """Detail panel renders per-issue lines instead of count."""
    state = make_state()
    state["issues_filed"] = [
        {
            "label": "Tech Debt",
            "title": "Extract helper for date parsing",
            "url": "https://github.com/test/test/issues/42",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T10:00:00-08:00",
        },
        {
            "label": "Flaky Test",
            "title": "test_timeout flakes on CI",
            "url": "https://github.com/test/test/issues/55",
            "phase": "flow-code",
            "phase_name": "Code",
            "timestamp": "2026-01-01T11:00:00-08:00",
        },
    ]
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_detail_panel(10)
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "#42" in text
    assert "Extract helper" in text
    assert "#55" in text
    assert "test_timeout" in text
    assert "Issues: 2 filed" not in text


def test_draw_detail_panel_single_issue():
    """Detail panel renders single issue correctly."""
    state = make_state()
    state["issues_filed"] = [
        {
            "label": "Flow",
            "title": "Process gap found",
            "url": "https://github.com/test/test/issues/10",
            "phase": "flow-learn",
            "phase_name": "Learn",
            "timestamp": "2026-01-01T12:00:00-08:00",
        },
    ]
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_detail_panel(10)
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "#10" in text
    assert "Process gap found" in text


def test_draw_detail_panel_issues_truncated_by_height():
    """Issues stop rendering when row reaches max_y - 2."""
    state = make_state()
    state["issues_filed"] = [
        {
            "label": "Tech Debt",
            "title": f"Issue {i}",
            "url": f"https://github.com/test/test/issues/{i}",
            "phase": "flow-code",
            "phase_name": "Code",
            "timestamp": "2026-01-01T10:00:00-08:00",
        }
        for i in range(1, 20)
    ]
    flow = _flow_from_state(state)
    # Terminal with room for timeline + some issues but not all 19
    stdscr = _make_stdscr(rows=28, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_detail_panel(10)
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    # Should render some issues but not all 19
    assert "#1" in text
    assert "#19" not in text


def test_draw_detail_panel_issue_title_truncated_by_safe_addstr():
    """Long issue titles are truncated by _safe_addstr to available width."""
    state = make_state()
    long_title = "A" * 100
    state["issues_filed"] = [
        {
            "label": "Tech Debt",
            "title": long_title,
            "url": "https://github.com/test/test/issues/1",
            "phase": "flow-code",
            "phase_name": "Code",
            "timestamp": "2026-01-01T10:00:00-08:00",
        },
    ]
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=40)
    app = _make_app(stdscr, flows=[flow])
    app._draw_detail_panel(10)
    # _safe_addstr truncates to max_x - col = 40 - 2 = 38
    issue_calls = [c for c in stdscr.addstr.call_args_list if "#1" in str(c[0][2])]
    assert len(issue_calls) >= 1
    rendered = issue_calls[0][0][2]
    assert len(rendered) <= 38  # max_x(40) - col(2)


def test_draw_list_view_blocked_shows_blocked_text():
    """Flow with blocked=True shows 'Blocked' in list row instead of elapsed time."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["_blocked"] = "2026-01-01T10:00:00-08:00"
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    # Find the flow list row (row 4)
    list_row_calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 4]
    assert list_row_calls, "Expected a call at row 4 for the flow list entry"
    list_row_text = list_row_calls[0][0][2]
    assert "Blocked" in list_row_text


def test_draw_list_view_blocked_row_uses_red():
    """Flow with blocked=True renders list row with COLOR_FAILED (red)."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["_blocked"] = "2026-01-01T10:00:00-08:00"
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app.use_color = True
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100):
        app._draw_list_view()
    list_row_calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 4 and "Blocked" in str(c[0][2])]
    assert list_row_calls, "Expected a row 4 call with 'Blocked' text"
    attr = list_row_calls[0][0][3]
    assert attr & (tui.COLOR_FAILED * 100), "Blocked row should use COLOR_FAILED (red)"


def test_draw_detail_panel_blocked_uses_red():
    """Flow with blocked=True renders in-progress phase [>] in red."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["_blocked"] = "2026-01-01T10:00:00-08:00"
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app.use_color = True
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100):
        app._draw_detail_panel(10)
    in_progress_calls = [c for c in stdscr.addstr.call_args_list if "[>]" in str(c[0][2])]
    assert len(in_progress_calls) >= 1
    for call in in_progress_calls:
        # Should use COLOR_FAILED (red) instead of COLOR_ACTIVE (yellow)
        assert call[0][3] == tui.COLOR_FAILED * 100 | curses.A_BOLD


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
    with patch("tui.detect_repo", return_value=None), patch("tui.subprocess.Popen") as mock_popen:
        app._open_issue(42)
        mock_popen.assert_not_called()


def test_open_issue_no_flows_with_detect():
    """Opens issue URL via detect_repo fallback when no flows exist."""
    app = _make_app(flows=[])
    with patch("tui.detect_repo", return_value="owner/repo"), patch("tui.subprocess.Popen") as mock_popen:
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


def test_I_key_opens_flow_issue():
    """'I' (capital) key extracts issue number from prompt and opens it."""
    state = make_state()
    state["prompt"] = "fix issue #42"
    app = _make_app(flows=[_flow_from_state(state)])
    with patch.object(app, "_open_issue") as mock_open:
        app._handle_list_input(ord("I"))
        mock_open.assert_called_once_with(42, repo="test/test")


def test_I_key_no_issue_in_prompt():
    """'I' key does nothing when prompt has no issue reference."""
    state = make_state()
    state["prompt"] = "add new feature"
    app = _make_app(flows=[_flow_from_state(state)])
    with patch.object(app, "_open_issue") as mock_open:
        app._handle_list_input(ord("I"))
        mock_open.assert_not_called()


def test_i_key_opens_issues_view():
    """'i' key switches to issues view."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    app._handle_list_input(ord("i"))
    assert app.view == "issues"


def test_open_flow_issue_no_flows():
    """_open_flow_issue does nothing when no flows exist."""
    app = _make_app(flows=[])
    with patch.object(app, "_open_issue") as mock_open:
        app._open_flow_issue()
        mock_open.assert_not_called()


def test_draw_list_view_footer_includes_issue_keys():
    """Footer includes [i] Issues and [I] Issue hints."""
    state = make_state()
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=120)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "[i] Issues" in text
    assert "[I] Issue" in text


# --- Issues view ---


def _make_issues_flow():
    """Build a flow with issues_filed for issues view tests."""
    state = make_state()
    state["issues_filed"] = [
        {
            "label": "Tech Debt",
            "title": "Extract date parser helper",
            "url": "https://github.com/test/test/issues/42",
            "phase": "flow-code-review",
            "phase_name": "Code Review",
            "timestamp": "2026-01-01T10:00:00-08:00",
        },
        {
            "label": "Flaky Test",
            "title": "test_timeout flakes on CI",
            "url": "https://github.com/test/test/issues/55",
            "phase": "flow-code",
            "phase_name": "Code",
            "timestamp": "2026-01-01T11:00:00-08:00",
        },
    ]
    return _flow_from_state(state)


def test_draw_issues_view_with_entries():
    """Issues view renders columnar display with Label, Ref, Phase, Title."""
    flow = _make_issues_flow()
    stdscr = _make_stdscr(rows=20, cols=100)
    app = _make_app(stdscr, flows=[flow])
    app.view = "issues"
    app._draw_issues_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "Tech Debt" in text
    assert "#42" in text
    assert "Code Review" in text
    assert "Extract date parser" in text
    assert "#55" in text
    assert "Flaky Test" in text


def test_draw_issues_view_empty():
    """Issues view shows empty message when no issues filed."""
    state = make_state()
    state["issues_filed"] = []
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=20, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app.view = "issues"
    app._draw_issues_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "No issues filed" in text


def test_draw_issues_view_no_flows():
    """Issues view switches back to list view when no flows exist."""
    stdscr = _make_stdscr()
    app = _make_app(stdscr, flows=[])
    app.view = "issues"
    app._draw_issues_view()
    assert app.view == "list"


def test_issues_view_navigate_up_down():
    """UP/DOWN changes issue_selected in issues view."""
    flow = _make_issues_flow()
    app = _make_app(flows=[flow])
    app.view = "issues"
    app.issue_selected = 0
    app._handle_issues_input(curses.KEY_DOWN)
    assert app.issue_selected == 1
    app._handle_issues_input(curses.KEY_UP)
    assert app.issue_selected == 0


def test_issues_view_navigate_bounds():
    """UP at top stays at 0, DOWN at bottom stays at max."""
    flow = _make_issues_flow()
    app = _make_app(flows=[flow])
    app.view = "issues"
    app.issue_selected = 0
    app._handle_issues_input(curses.KEY_UP)
    assert app.issue_selected == 0
    app.issue_selected = 1
    app._handle_issues_input(curses.KEY_DOWN)
    assert app.issue_selected == 1


def test_issues_view_enter_opens_url():
    """ENTER opens selected issue URL via subprocess."""
    flow = _make_issues_flow()
    app = _make_app(flows=[flow])
    app.view = "issues"
    app.issue_selected = 0
    with patch("tui.subprocess.Popen") as mock_popen:
        app._handle_issues_input(ord("\n"))
        mock_popen.assert_called_once()
        args = mock_popen.call_args[0][0]
        assert args[0] == "open"
        assert "issues/42" in args[1]


def test_issues_view_esc_returns_to_list():
    """ESC in issues view returns to list view."""
    app = _make_app()
    app.view = "issues"
    app._handle_input(27)
    assert app.view == "list"


def test_issues_view_no_issues_input():
    """Input handling does nothing for navigation when no issues exist."""
    state = make_state()
    state["issues_filed"] = []
    flow = _flow_from_state(state)
    app = _make_app(flows=[flow])
    app.view = "issues"
    app.issue_selected = 0
    app._handle_issues_input(curses.KEY_DOWN)
    assert app.issue_selected == 0


def test_issues_view_selected_marker():
    """Selected issue has the marker indicator."""
    flow = _make_issues_flow()
    stdscr = _make_stdscr(rows=20, cols=100)
    app = _make_app(stdscr, flows=[flow])
    app.view = "issues"
    app.issue_selected = 0
    app._draw_issues_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    # First issue row should be bold (selected)
    issue_calls = [c for c in calls if "#42" in c]
    assert len(issue_calls) >= 1


def test_issues_view_clamps_selection():
    """Selected index is clamped when it exceeds issues count."""
    flow = _make_issues_flow()
    stdscr = _make_stdscr(rows=20, cols=100)
    app = _make_app(stdscr, flows=[flow])
    app.view = "issues"
    app.issue_selected = 99  # Way beyond 2 issues
    app._draw_issues_view()
    assert app.issue_selected == 1  # Clamped to last index


def test_issues_view_height_overflow():
    """Issues view stops rendering when terminal is too small."""
    state = make_state()
    state["issues_filed"] = [
        {
            "label": "Tech Debt",
            "title": f"Issue {i}",
            "url": f"https://github.com/test/test/issues/{i}",
            "phase": "flow-code",
            "phase_name": "Code",
            "timestamp": "2026-01-01T10:00:00-08:00",
        }
        for i in range(1, 20)
    ]
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=10, cols=100)
    app = _make_app(stdscr, flows=[flow])
    app.view = "issues"
    app._draw_issues_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "#1" in text
    assert "#19" not in text


def test_issues_view_width_truncation():
    """Long issue lines are truncated by _safe_addstr to terminal width."""
    state = make_state()
    state["issues_filed"] = [
        {
            "label": "Tech Debt",
            "title": "A" * 100,
            "url": "https://github.com/test/test/issues/1",
            "phase": "flow-code",
            "phase_name": "Code",
            "timestamp": "2026-01-01T10:00:00-08:00",
        },
    ]
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=20, cols=50)
    app = _make_app(stdscr, flows=[flow])
    app.view = "issues"
    app._draw_issues_view()
    # _safe_addstr truncates to max_x - col = 50 - 2 = 48
    issue_calls = [c for c in stdscr.addstr.call_args_list if "#1" in str(c[0][2])]
    assert len(issue_calls) >= 1
    rendered = issue_calls[0][0][2]
    assert len(rendered) <= 48  # max_x(50) - col(2)


def test_handle_issues_input_no_flows():
    """_handle_issues_input does nothing when no flows exist."""
    app = _make_app(flows=[])
    app.view = "issues"
    app._handle_issues_input(curses.KEY_DOWN)
    assert app.issue_selected == 0


def test_handle_input_dispatches_to_issues_view():
    """Non-quit/non-escape keys in issues view dispatch to _handle_issues_input."""
    flow = _make_issues_flow()
    app = _make_app(flows=[flow])
    app.view = "issues"
    with patch.object(app, "_handle_issues_input") as mock_issues:
        app._handle_input(curses.KEY_UP)
        mock_issues.assert_called_once_with(curses.KEY_UP)


def test_run_loop_draws_issues_view():
    """Run loop calls _draw_issues_view when view is 'issues'."""
    flow = _make_issues_flow()
    stdscr = _make_stdscr(rows=20, cols=80)
    stdscr.getch.side_effect = [ord("q")]
    app = _make_app(stdscr, flows=[flow])
    app.view = "issues"
    with (
        patch("tui.curses.curs_set"),
        patch.object(app, "_init_colors"),
        patch.object(app, "_draw_issues_view") as mock_draw,
    ):
        app.run()
        mock_draw.assert_called()


# --- Tab bar and orchestration view ---


def _make_orch_data(items=None, elapsed="4h 12m", completed_count=0, failed_count=0, total=0, is_running=True):
    """Build a minimal orchestration summary dict for tests."""
    return {
        "elapsed": elapsed,
        "completed_count": completed_count,
        "failed_count": failed_count,
        "total": total if total else len(items or []),
        "is_running": is_running,
        "items": items or [],
    }


def _make_orch_item(issue_number, title, icon="\u00b7", status="pending", elapsed="", pr_url=None, reason=None):
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


def test_header_branding_cyan_bold():
    """Header branding renders with cyan color pair OR'd with A_BOLD."""
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[])
    app.use_color = True
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100):
        app._draw_header()
    branding_calls = [c for c in stdscr.addstr.call_args_list if "FLOW v" in str(c[0][2])]
    assert len(branding_calls) == 1
    assert branding_calls[0][0][3] == tui.COLOR_HEADER * 100 | curses.A_BOLD


# --- _draw_header: account metrics ---


def test_draw_header_metrics_right_aligned():
    """Header shows cost and rate limits right-aligned on row 0."""
    stdscr = _make_stdscr(rows=10, cols=100)
    app = _make_app(stdscr)
    app.metrics = {"cost_monthly": "12.50", "rl_5h": 45, "rl_7d": 32, "stale": False}
    app._draw_header()
    calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 0]
    texts = [str(c[0][2]) for c in calls]
    combined = " ".join(texts)
    assert "$12.50/mo" in combined
    assert "5h:45%" in combined
    assert "7d:32%" in combined


def test_draw_header_metrics_yellow_threshold():
    """Rate limits at 70-89% render with COLOR_ACTIVE (yellow)."""
    stdscr = _make_stdscr(rows=10, cols=100)
    app = _make_app(stdscr)
    app.use_color = True
    app.metrics = {"cost_monthly": "0.00", "rl_5h": 75, "rl_7d": 85, "stale": False}
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100):
        app._draw_header()
    calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 0]
    yellow_calls = [c for c in calls if c[0][3] == tui.COLOR_ACTIVE * 100]
    assert len(yellow_calls) >= 2, "Both rate limits at 70-89% should use yellow"


def test_draw_header_metrics_red_threshold():
    """Rate limits at >=90% render with COLOR_FAILED (red)."""
    stdscr = _make_stdscr(rows=10, cols=100)
    app = _make_app(stdscr)
    app.use_color = True
    app.metrics = {"cost_monthly": "0.00", "rl_5h": 92, "rl_7d": 95, "stale": False}
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100):
        app._draw_header()
    calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 0]
    red_calls = [c for c in calls if c[0][3] == tui.COLOR_FAILED * 100]
    assert len(red_calls) >= 2, "Both rate limits at >=90% should use red"


def test_draw_header_metrics_stale():
    """Stale metrics show 5h:-- 7d:-- dimmed."""
    stdscr = _make_stdscr(rows=10, cols=100)
    app = _make_app(stdscr)
    app.metrics = {"cost_monthly": "5.00", "rl_5h": None, "rl_7d": None, "stale": True}
    app._draw_header()
    calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 0]
    texts = [str(c[0][2]) for c in calls]
    combined = " ".join(texts)
    assert "5h:--" in combined
    assert "7d:--" in combined
    dim_calls = [c for c in calls if c[0][3] == curses.A_DIM and ("5h:--" in str(c[0][2]) or "7d:--" in str(c[0][2]))]
    assert len(dim_calls) >= 1


def test_draw_header_no_metrics():
    """Header renders without error when metrics is None."""
    stdscr = _make_stdscr(rows=10, cols=80)
    app = _make_app(stdscr)
    app.metrics = None
    app._draw_header()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "FLOW" in text


def test_draw_header_metrics_narrow_terminal():
    """Metrics are omitted on narrow terminals without crash."""
    stdscr = _make_stdscr(rows=10, cols=30)
    app = _make_app(stdscr)
    app.metrics = {"cost_monthly": "10.00", "rl_5h": 50, "rl_7d": 60, "stale": False}
    app._draw_header()
    calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 0]
    texts = [str(c[0][2]) for c in calls]
    combined = " ".join(texts)
    # Metrics should not appear — terminal too narrow
    assert "$10.00/mo" not in combined


def test_draw_header_metrics_narrow_terminal_stale():
    """Stale metrics are omitted on narrow terminals without crash."""
    stdscr = _make_stdscr(rows=10, cols=30)
    app = _make_app(stdscr)
    app.metrics = {"cost_monthly": "5.00", "rl_5h": None, "rl_7d": None, "stale": True}
    app._draw_header()
    calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 0]
    texts = [str(c[0][2]) for c in calls]
    combined = " ".join(texts)
    assert "$5.00/mo" not in combined


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


def test_draw_tab_bar_active_tab_uses_blue():
    """Active tab title renders with COLOR_LINK (blue)."""
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[])
    app.active_tab = 0
    app.use_color = True
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100):
        app._draw_tab_bar(2)
    active_tab_calls = [c for c in stdscr.addstr.call_args_list if "Active Flows" in str(c[0][2])]
    assert active_tab_calls, "Expected a call rendering 'Active Flows'"
    attr = active_tab_calls[0][0][3]
    assert attr & (tui.COLOR_LINK * 100), "Active tab should use COLOR_LINK (blue)"
    assert attr & curses.A_BOLD, "Active tab should also be bold"


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
        _make_orch_item(42, "Add PDF export", icon="\u2713", status="completed", elapsed="1h 24m"),
        _make_orch_item(43, "Fix login", icon="\u2717", status="failed", elapsed="1h 2m"),
        _make_orch_item(45, "Update hooks", icon="\u25b6", status="in_progress", elapsed="38m"),
        _make_orch_item(46, "Add rate limiting", icon="\u00b7"),
    ]
    orch = _make_orch_data(items=items, completed_count=1, failed_count=1, is_running=True)
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


def test_draw_orchestration_view_title_expands_on_wide_terminal():
    """Orchestration title column padding scales with terminal width on wide terminals."""
    short_title = "Fix login"  # 9 chars — well under any width
    items = [_make_orch_item(42, short_title, icon="\u25b6", status="in_progress", elapsed="38m")]
    orch = _make_orch_data(items=items)
    stdscr = _make_stdscr(rows=30, cols=120)
    app = _make_app(stdscr, flows=[], orch_data=orch)
    app.active_tab = 1
    app._draw_orchestration_view()
    # Find the item row with #42
    item_calls = [c for c in stdscr.addstr.call_args_list if "#42" in str(c[0][2])]
    assert item_calls, "Expected an orchestration item call with #42"
    item_text = item_calls[0][0][2]
    # At 120 cols, the title column should be padded wider than the default 30
    # orch_title_width = max(30, 120 - 44) = 76
    # The title "Fix login" (9 chars) should be padded to 76 chars in the column
    # Verify the elapsed "38m" appears further right than it would at 30-char padding
    elapsed_pos = item_text.index("38m")
    # With 30-char title column: prefix (~12) + 30 + "  " = ~44
    # With 76-char title column: prefix (~12) + 76 + "  " = ~90
    assert elapsed_pos > 50, f"Elapsed '38m' should be pushed right by wider title column, found at pos {elapsed_pos}"


def test_orch_completed_item_green():
    """Completed orchestration item renders with green color pair."""
    items = [_make_orch_item(42, "Done task", icon="\u2713", status="completed")]
    orch = _make_orch_data(items=items, completed_count=1)
    stdscr = _make_stdscr(rows=30, cols=80)
    app = _make_app(stdscr, flows=[], orch_data=orch)
    app.active_tab = 1
    app.use_color = True
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100):
        app._draw_orchestration_view()
    item_calls = [c for c in stdscr.addstr.call_args_list if "#42" in str(c[0][2])]
    assert len(item_calls) == 1
    attr = item_calls[0][0][3]
    expected = tui.COLOR_COMPLETE * 100 | curses.A_BOLD
    assert attr == expected


def test_orch_failed_item_red():
    """Failed orchestration item renders with red color pair."""
    items = [_make_orch_item(43, "Broken task", icon="\u2717", status="failed")]
    orch = _make_orch_data(items=items, failed_count=1)
    stdscr = _make_stdscr(rows=30, cols=80)
    app = _make_app(stdscr, flows=[], orch_data=orch)
    app.active_tab = 1
    app.use_color = True
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100):
        app._draw_orchestration_view()
    item_calls = [c for c in stdscr.addstr.call_args_list if "#43" in str(c[0][2])]
    assert len(item_calls) == 1
    attr = item_calls[0][0][3]
    expected = tui.COLOR_FAILED * 100 | curses.A_BOLD
    assert attr == expected


def test_orch_in_progress_item_yellow():
    """In-progress orchestration item renders with yellow color pair."""
    items = [_make_orch_item(45, "Active task", icon="\u25b6", status="in_progress")]
    orch = _make_orch_data(items=items)
    stdscr = _make_stdscr(rows=30, cols=80)
    app = _make_app(stdscr, flows=[], orch_data=orch)
    app.active_tab = 1
    app.use_color = True
    with patch("tui.curses.color_pair", side_effect=lambda p: p * 100):
        app._draw_orchestration_view()
    item_calls = [c for c in stdscr.addstr.call_args_list if "#45" in str(c[0][2])]
    assert len(item_calls) == 1
    attr = item_calls[0][0][3]
    expected = tui.COLOR_ACTIVE * 100 | curses.A_BOLD
    assert attr == expected


def test_orch_pending_item_dim():
    """Pending orchestration item renders with A_DIM."""
    items = [
        _make_orch_item(45, "Active task", icon="\u25b6", status="in_progress"),
        _make_orch_item(46, "Waiting task"),
    ]
    orch = _make_orch_data(items=items)
    stdscr = _make_stdscr(rows=30, cols=80)
    app = _make_app(stdscr, flows=[], orch_data=orch)
    app.active_tab = 1
    app.orch_selected = 0
    app._draw_orchestration_view()
    item_calls = [c for c in stdscr.addstr.call_args_list if "#46" in str(c[0][2])]
    assert len(item_calls) == 1
    assert item_calls[0][0][3] == curses.A_DIM


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
        _make_orch_item(42, "Add PDF export", icon="\u25b6", status="in_progress", elapsed="38m"),
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
    with (
        patch("tui.curses.curs_set"),
        patch.object(app, "_init_colors"),
        patch.object(app, "_draw_orchestration_view") as mock_draw,
    ):
        app.run()
        mock_draw.assert_called()


def test_orchestration_tab_count_in_tab_bar():
    """Tab bar shows Orchestration (N/M) when running."""
    items = [
        _make_orch_item(42, "A", icon="\u2713", status="completed"),
        _make_orch_item(43, "B", icon="\u2717", status="failed"),
        _make_orch_item(44, "C"),
    ]
    orch = _make_orch_data(items=items, completed_count=1, failed_count=1, is_running=True)
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
    with patch("tui.detect_repo", return_value=None), patch("tui.subprocess.Popen") as mock_popen:
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
        _make_orch_item(43, "Fix login", icon="\u2717", status="failed", reason="CI failed after 3 attempts"),
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
        _make_orch_item(
            42,
            "Add PDF export",
            icon="\u2713",
            status="completed",
            elapsed="1h 24m",
            pr_url="https://github.com/test/test/pull/58",
        ),
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
        _make_orch_item(
            42,
            "Done",
            icon="\u2713",
            status="completed",
            elapsed="1h 24m",
            pr_url="https://github.com/test/test/pull/58",
        ),
    ]
    orch = _make_orch_data(items=items, completed_count=1)
    stdscr = _make_stdscr(rows=30, cols=80)
    app = _make_app(stdscr, flows=[], orch_data=orch)
    app.active_tab = 1
    app._draw_orchestration_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "PR 58" in text


def test_orch_view_item_with_pr_url_trailing_slash():
    """PR number extraction handles trailing slash in pr_url."""
    items = [
        _make_orch_item(
            42,
            "Done",
            icon="\u2713",
            status="completed",
            elapsed="1h 24m",
            pr_url="https://github.com/test/test/pull/58/",
        ),
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
        "queue": [
            {
                "issue_number": 42,
                "title": "A",
                "status": "pending",
                "started_at": None,
                "completed_at": None,
                "outcome": None,
                "pr_url": None,
                "branch": None,
                "reason": None,
            }
        ],
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
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
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


# --- tasks view ---


def test_handle_list_input_t_switches_to_tasks_view():
    """Pressing 't' in list view switches to tasks view."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    app.view = "list"
    app._handle_list_input(ord("t"))
    assert app.view == "tasks"


def test_draw_tasks_view_renders_plan_content(tmp_path):
    """Tasks view renders plan file content."""
    plan_file = tmp_path / "plan.md"
    plan_file.write_text("# Plan\n\n## Tasks\n\n- Task 1\n- Task 2\n")
    state = make_state()
    state["files"]["plan"] = str(plan_file)
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=30, cols=80)
    app = _make_app(stdscr, flows=[flow], root=tmp_path)
    app.view = "tasks"
    app._draw_tasks_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "Task 1" in text


def test_draw_tasks_view_no_plan():
    """Tasks view shows 'No plan file.' when plan_path is None."""
    state = make_state()
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=30, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app.view = "tasks"
    app._draw_tasks_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "No plan file" in text


def test_handle_input_escape_tasks_returns_to_list():
    """Escape from tasks view returns to list view."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    app.view = "tasks"
    app._handle_input(27)
    assert app.view == "list"


def test_draw_list_view_footer_includes_tasks():
    """Footer in list view includes [t] Tasks shortcut."""
    state = make_state()
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=120)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "[t] Tasks" in text


def test_run_loop_draws_tasks_view():
    """Run loop dispatches to _draw_tasks_view when view is 'tasks'."""
    state = make_state()
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=30, cols=80)
    stdscr.getch.side_effect = [ord("q")]
    app = _make_app(stdscr, flows=[flow])
    app.view = "tasks"
    with (
        patch("tui.curses.curs_set"),
        patch.object(app, "_init_colors"),
        patch.object(app, "_draw_tasks_view") as mock_draw,
    ):
        app.run()
        mock_draw.assert_called()


def test_draw_tasks_view_missing_file(tmp_path):
    """Tasks view shows 'No plan file.' when plan file path does not exist."""
    state = make_state()
    state["files"]["plan"] = str(tmp_path / "nonexistent.md")
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=30, cols=80)
    app = _make_app(stdscr, flows=[flow], root=tmp_path)
    app.view = "tasks"
    app._draw_tasks_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "No plan file" in text


def test_draw_tasks_view_no_flows():
    """Tasks view returns to list when no flows exist."""
    stdscr = _make_stdscr(rows=30, cols=80)
    app = _make_app(stdscr, flows=[])
    app.view = "tasks"
    app._draw_tasks_view()
    assert app.view == "list"


def test_draw_tasks_view_content_overflow(tmp_path):
    """Tasks view truncates plan content that exceeds screen height."""
    plan_file = tmp_path / "plan.md"
    plan_file.write_text("\n".join(f"Line {i}" for i in range(100)))
    state = make_state()
    state["files"]["plan"] = str(plan_file)
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=10, cols=80)
    app = _make_app(stdscr, flows=[flow], root=tmp_path)
    app.view = "tasks"
    app._draw_tasks_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    text = " ".join(calls)
    assert "Line 0" in text
    assert "Line 99" not in text


def test_handle_input_tasks_view_ignores_unknown_keys():
    """Unknown keys in tasks view are ignored (view stays 'tasks')."""
    state = make_state()
    app = _make_app(flows=[_flow_from_state(state)])
    app.view = "tasks"
    app._handle_input(ord("x"))
    assert app.view == "tasks"


# --- List view annotation ---


def test_draw_list_view_shows_annotation():
    """List view flow row renders phase annotation inline with phase info."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 2
    state["code_tasks_total"] = 5
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=120)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    # The flow list row is at row 4 (after header). Find that specific row.
    row_4_calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 4]
    row_4_text = " ".join(str(c[0][2]) for c in row_4_calls)
    assert "task 3 of 5" in row_4_text


def test_draw_list_view_no_annotation_when_empty():
    """List view does not add parens when annotation is empty."""
    state = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    calls = [str(c) for c in stdscr.addstr.call_args_list]
    # Find the flow row (contains "Start")
    flow_row_calls = [c for c in calls if "Start" in c and "Test Feature" in c]
    assert len(flow_row_calls) == 1
    # Should not contain empty parens "()"
    assert "()" not in flow_row_calls[0]


def test_draw_list_view_columns_aligned_across_flows():
    """Phase, elapsed, and PR columns align vertically across flows with varying phase info widths."""
    import re

    # Flow 1: Code Review with annotation (longest phase_info)
    state1 = make_state(
        current_phase="flow-code-review",
        phase_statuses={
            "flow-start": "complete",
            "flow-plan": "complete",
            "flow-code": "complete",
            "flow-code-review": "in_progress",
        },
    )
    state1["branch"] = "alpha-feature"
    state1["code_review_step"] = 2

    # Flow 2: Start phase (short phase_info)
    state2 = make_state(
        current_phase="flow-start",
        phase_statuses={"flow-start": "in_progress"},
    )
    state2["branch"] = "beta-feature"

    # Flow 3: Code with task annotation (medium phase_info)
    state3 = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state3["branch"] = "gamma-feature"
    state3["code_task"] = 2
    state3["code_tasks_total"] = 5

    flows = [_flow_from_state(s) for s in [state1, state2, state3]]
    stdscr = _make_stdscr(rows=40, cols=120)
    app = _make_app(stdscr, flows=flows)
    app._draw_list_view()

    # Extract flow list rows (rows 4, 5, 6)
    row_texts = []
    for row_num in (4, 5, 6):
        row_calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == row_num]
        assert row_calls, f"Expected addstr call at row {row_num}"
        row_texts.append(row_calls[0][0][2])

    # Find where the phase number starts (pattern: "N: " where N is 1-6)
    phase_positions = []
    for text in row_texts:
        match = re.search(r"\d: ", text)
        assert match, f"Expected phase number pattern in: {text}"
        phase_positions.append(match.start())

    # All phase columns must start at the same position
    assert len(set(phase_positions)) == 1, f"Phase columns misaligned: positions={phase_positions}"

    # Find where "PR #" starts (all flows have pr_number=1)
    pr_positions = []
    for text in row_texts:
        pr_idx = text.find("PR #")
        assert pr_idx >= 0, f"Expected 'PR #' in: {text}"
        pr_positions.append(pr_idx)

    # All PR columns must start at the same position
    assert len(set(pr_positions)) == 1, f"PR columns misaligned: positions={pr_positions}"


# --- List view phase elapsed ---


def test_draw_list_view_shows_phase_elapsed():
    """List view shows phase elapsed as a separate column before total elapsed."""
    from datetime import datetime

    from flow_utils import PACIFIC

    # Start 30m ago, but only 3m in the current phase — values must differ
    now = datetime(2026, 1, 1, 0, 30, 0, tzinfo=PACIFIC)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["started_at"] = "2026-01-01T00:00:00-08:00"
    state["phases"]["flow-code"]["session_started_at"] = "2026-01-01T00:27:00-08:00"
    state["phases"]["flow-code"]["cumulative_seconds"] = 0
    from tui_data import flow_summary

    flow = flow_summary(state, now=now)
    assert flow["phase_elapsed"] == "3m"
    assert flow["elapsed"] == "30m"
    stdscr = _make_stdscr(rows=40, cols=120)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    list_row_calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 4]
    assert list_row_calls, "Expected addstr call at row 4"
    row_text = list_row_calls[0][0][2]
    # Row must contain BOTH "3m" (phase elapsed) and "30m" (total elapsed)
    assert "3m" in row_text, f"Phase elapsed '3m' not found in: {row_text}"
    assert "30m" in row_text, f"Total elapsed '30m' not found in: {row_text}"
    # Phase elapsed must appear before total elapsed
    idx_phase = row_text.index("3m")
    idx_total = row_text.index("30m")
    assert idx_phase < idx_total, f"Phase elapsed should appear before total: phase@{idx_phase} total@{idx_total}"


def test_draw_list_view_blocked_hides_phase_elapsed():
    """Blocked flow shows 'Blocked' and suppresses the phase elapsed column."""
    from datetime import datetime

    from flow_utils import PACIFIC

    now = datetime(2026, 1, 1, 0, 10, 0, tzinfo=PACIFIC)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["_blocked"] = "2026-01-01T10:00:00-08:00"
    state["phases"]["flow-code"]["session_started_at"] = "2026-01-01T00:00:00-08:00"
    state["phases"]["flow-code"]["cumulative_seconds"] = 300
    from tui_data import flow_summary

    flow = flow_summary(state, now=now)
    # phase_elapsed is "15m" (300s + 600s live) — but blocked should suppress it
    assert flow["phase_elapsed"] == "15m"
    stdscr = _make_stdscr(rows=40, cols=120)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    list_row_calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 4]
    assert list_row_calls, "Expected addstr call at row 4"
    row_text = list_row_calls[0][0][2]
    assert "Blocked" in row_text
    # "15m" (phase elapsed) must NOT appear in the row when blocked
    assert "15m" not in row_text, f"Phase elapsed '15m' should be suppressed when blocked: {row_text}"


def test_draw_detail_panel_in_progress_shows_time():
    """Detail panel [>] line includes formatted time before annotation."""
    from datetime import datetime

    from flow_utils import PACIFIC

    now = datetime(2026, 1, 1, 0, 5, 0, tzinfo=PACIFIC)
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    state["code_task"] = 2
    state["code_tasks_total"] = 5
    state["phases"]["flow-code"]["session_started_at"] = "2026-01-01T00:00:00-08:00"
    state["phases"]["flow-code"]["cumulative_seconds"] = 0
    from tui_data import flow_summary

    flow = flow_summary(state, now=now)
    stdscr = _make_stdscr(rows=40, cols=80)
    app = _make_app(stdscr, flows=[flow])
    app._draw_detail_panel(10)
    # Find the [>] line for the in-progress phase
    in_progress_calls = [c for c in stdscr.addstr.call_args_list if "[>]" in str(c[0][2])]
    assert len(in_progress_calls) >= 1
    line_text = in_progress_calls[0][0][2]
    # Should contain time ("5m") before the annotation
    assert "5m" in line_text


# --- List view column headers ---


def test_draw_list_view_shows_column_headers():
    """Active Flows table renders a dim header row at row 3 with column labels."""
    state = make_state(
        current_phase="flow-code",
        phase_statuses={"flow-start": "complete", "flow-plan": "complete", "flow-code": "in_progress"},
    )
    flow = _flow_from_state(state)
    stdscr = _make_stdscr(rows=40, cols=120)
    app = _make_app(stdscr, flows=[flow])
    app._draw_list_view()
    # Find addstr calls at row 3 — the header row
    hdr_calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 3]
    assert hdr_calls, "Expected at least one addstr call at row 3"
    # The last call at row 3 is the column header (overwrites the separator)
    hdr_text = hdr_calls[-1][0][2]
    hdr_attr = hdr_calls[-1][0][3] if len(hdr_calls[-1][0]) > 3 else 0
    assert "Feature" in hdr_text, f"Expected 'Feature' in header: {hdr_text}"
    assert "Phase" in hdr_text, f"Expected 'Phase' in header: {hdr_text}"
    assert "Total" in hdr_text, f"Expected 'Total' in header: {hdr_text}"
    assert hdr_attr == curses.A_DIM, f"Expected A_DIM attribute, got: {hdr_attr}"


def test_draw_list_view_empty_no_column_headers():
    """Empty state (no flows) does not render column headers at row 3."""
    stdscr = _make_stdscr(rows=40, cols=120)
    app = _make_app(stdscr, flows=[])
    app._draw_list_view()
    # Row 3 should only have the separator from _draw_header, not a column header
    hdr_calls = [c for c in stdscr.addstr.call_args_list if c[0][0] == 3]
    for call in hdr_calls:
        text = call[0][2]
        assert "Feature" not in text, f"Column header should not render when no flows: {text}"
