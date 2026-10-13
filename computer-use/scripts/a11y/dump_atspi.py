#!/usr/bin/env python3
"""Dump Linux AT-SPI accessibility tree of gcu-fixture application to JSON.

Dependencies:
  - Python 3 standard library
  - PyGObject (`python3-gi`)
  - AT-SPI 2.0 typelib (`gir1.2-atspi-2.0`)
"""

import argparse
import json
import os
import sys
import time

try:
    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi
except Exception as err:
    print(
        f"ERROR: Failed to import Atspi 2.0 via gi: {err}\n"
        "Please ensure 'python3-gi' and 'gir1.2-atspi-2.0' are installed.",
        file=sys.stderr,
    )
    sys.exit(2)


def dump_node(acc, depth=0, max_depth=60):
    """Recursively serializes an Atspi.Accessible into a JSON-friendly dict."""
    if acc is None or depth > max_depth:
        return None

    # Role
    try:
        role_enum = acc.get_role()
        role_name = getattr(role_enum, "value_name", str(role_enum))
        role_nick = getattr(role_enum, "value_nick", str(role_enum))
        role_str = acc.get_role_name() or ""
        role_id = int(role_enum)
    except Exception:
        role_name = "ATSPI_ROLE_UNKNOWN"
        role_nick = "unknown"
        role_str = "unknown"
        role_id = 0

    # Name and description
    try:
        name = acc.get_name() or ""
    except Exception:
        name = ""

    try:
        desc = acc.get_description() or ""
    except Exception:
        desc = ""

    # States
    states = []
    state_nicks = []
    try:
        state_set = acc.get_state_set()
        if state_set:
            for s in state_set.get_states():
                states.append(getattr(s, "value_name", str(s)))
                state_nicks.append(getattr(s, "value_nick", str(s)))
    except Exception:
        pass

    # Actions
    actions = []
    try:
        n_actions = acc.get_n_actions()
        for i in range(n_actions):
            act_name = acc.get_action_name(i)
            if act_name:
                actions.append(act_name)
    except Exception:
        pass

    # Extents
    extents = {"x": 0, "y": 0, "width": 0, "height": 0}
    try:
        rect = acc.get_extents(Atspi.CoordType.SCREEN)
        if rect:
            extents = {
                "x": int(rect.x),
                "y": int(rect.y),
                "width": int(rect.width),
                "height": int(rect.height),
            }
    except Exception:
        pass

    # Attributes
    attributes = {}
    try:
        attrs = acc.get_attributes()
        if attrs:
            for k, v in attrs.items():
                attributes[str(k)] = str(v)
    except Exception:
        pass

    # Children
    children = []
    try:
        c_count = acc.get_child_count()
        for i in range(c_count):
            child_acc = acc.get_child_at_index(i)
            if child_acc:
                c_dump = dump_node(child_acc, depth + 1, max_depth)
                if c_dump is not None:
                    children.append(c_dump)
    except Exception:
        pass

    return {
        "role": role_name,
        "role_nick": role_nick,
        "role_name": role_str,
        "role_id": role_id,
        "name": name,
        "description": desc,
        "states": states,
        "state_nicks": state_nicks,
        "actions": actions,
        "extents": extents,
        "attributes": attributes,
        "children": children,
    }


def trigger_screen_reader_signal():
    """Toggle ScreenReaderEnabled to ensure accesskit_unix receives a PropertiesChanged signal."""
    import subprocess
    try:
        subprocess.run(
            ["busctl", "--user", "set-property", "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Status", "ScreenReaderEnabled", "b", "false"],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        time.sleep(0.05)
        subprocess.run(
            ["busctl", "--user", "set-property", "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Status", "ScreenReaderEnabled", "b", "true"],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except Exception:
        pass


def find_fixture_app(timeout=30.0, retry_interval=0.5):
    """Find the gcu-fixture application node in AT-SPI desktop."""
    start = time.time()
    last_err = None
    found_names = []
    poked_after_start = False

    print(f"Connecting to AT-SPI desktop (timeout={timeout}s)...")
    while time.time() - start < timeout:
        # Trigger signal after 1.0s to ensure accesskit_unix gets the activation event
        if not poked_after_start and (time.time() - start) > 1.0:
            trigger_screen_reader_signal()
            poked_after_start = True

        try:
            desktop = Atspi.get_desktop(0)
            if desktop is None:
                time.sleep(retry_interval)
                continue

            child_count = desktop.get_child_count()
            found_names.clear()
            for i in range(child_count):
                child = desktop.get_child_at_index(i)
                if not child:
                    continue
                name = child.get_name() or ""
                toolkit = ""
                try:
                    toolkit = child.get_toolkit_name() or ""
                except Exception:
                    pass
                found_names.append(f"'{name}' (toolkit={toolkit})")

                lower_name = name.lower()
                if (
                    "gcu-fixture" in lower_name
                    or "gcu fixture" in lower_name
                    or "gcu_fixture" in lower_name
                    or "fixture" in lower_name
                ):
                    print(f"Found fixture application: {name} (index={i})")
                    return child

                # Inspect top-level window titles under this app
                try:
                    for w_idx in range(child.get_child_count()):
                        w = child.get_child_at_index(w_idx)
                        if w:
                            w_name = (w.get_name() or "").lower()
                            if "fixture" in w_name or "gcu" in w_name:
                                print(
                                    f"Found fixture via window title '{w.get_name()}' under app '{name}'"
                                )
                                return child
                except Exception:
                    pass
        except Exception as e:
            last_err = e

        time.sleep(retry_interval)

    err_msg = (
        f"Fixture application not found in AT-SPI desktop within {timeout}s.\n"
        f"Last error: {last_err}\n"
        f"Applications found on desktop: {found_names}"
    )
    raise TimeoutError(err_msg)


def find_modal_trigger(acc):
    """Recursively search for the 'open_modal_button' or 'Open Modal' button."""
    if acc is None:
        return None
    name = (acc.get_name() or "").lower()
    if (
        "open_modal_button" in name
        or "open modal" in name
        or "打开弹窗" in name
    ):
        return acc

    for i in range(acc.get_child_count()):
        child = acc.get_child_at_index(i)
        if child:
            found = find_modal_trigger(child)
            if found:
                return found
    return None


def trigger_modal(app_acc):
    """Attempts to trigger the modal dialog button via AT-SPI action or click."""
    btn = find_modal_trigger(app_acc)
    if not btn:
        print("Warning: open_modal_button not found in accessibility tree", file=sys.stderr)
        return False

    btn_name = btn.get_name() or "unnamed button"
    print(f"Attempting to trigger modal button '{btn_name}'...")
    triggered = False

    # 1. Try action interface
    try:
        n_act = btn.get_n_actions()
        if n_act > 0:
            act_name = btn.get_action_name(0)
            print(f"Executing action 0 ('{act_name}') via Accessible.do_action...")
            triggered = btn.do_action(0)
            print(f"do_action result: {triggered}")
    except Exception as e:
        print(f"Accessible.do_action failed: {e}", file=sys.stderr)

    # 2. If do_action returned false or threw, fallback to mouse click at extents center
    if not triggered:
        try:
            rect = btn.get_extents(Atspi.CoordType.SCREEN)
            if rect and rect.width > 0 and rect.height > 0:
                cx = int(rect.x + rect.width / 2)
                cy = int(rect.y + rect.height / 2)
                print(f"Generating AT-SPI mouse click at ({cx}, {cy})...")
                Atspi.generate_mouse_event(cx, cy, "b1c")
                triggered = True
        except Exception as e:
            print(f"Atspi.generate_mouse_event failed: {e}", file=sys.stderr)

    return triggered


def print_tree_summary(node, indent=0, max_depth=4):
    """Prints a concise hierarchical summary of the tree for CI inspection."""
    if node is None or indent > max_depth * 2:
        return
    prefix = " " * indent
    name_str = f" '{node['name']}'" if node["name"] else ""
    print(f"{prefix}- [{node['role_nick']}]{name_str}")
    for child in node.get("children", []):
        print_tree_summary(child, indent + 2, max_depth)


def main():
    parser = argparse.ArgumentParser(description="Dump AT-SPI tree of gcu-fixture to JSON.")
    parser.add_argument(
        "--output",
        "-o",
        default="/tmp/fixture_tree.json",
        help="Target output JSON file path (default: /tmp/fixture_tree.json)",
    )
    parser.add_argument(
        "--timeout",
        "-t",
        type=float,
        default=30.0,
        help="Timeout in seconds to wait for application to appear (default: 30.0)",
    )
    parser.add_argument(
        "--retry-interval",
        type=float,
        default=0.5,
        help="Polling interval in seconds (default: 0.5)",
    )
    parser.add_argument(
        "--no-modal",
        action="store_true",
        help="Skip triggering modal dialog capture",
    )
    parser.add_argument(
        "--single-tree",
        action="store_true",
        help="Output raw tree node at root instead of {tree, modal_tree}",
    )
    parser.add_argument(
        "positional_output",
        nargs="?",
        default=None,
        help="Positional argument for output path (takes precedence if given)",
    )

    args = parser.parse_args()
    out_path = args.positional_output or args.output

    try:
        app_acc = find_fixture_app(timeout=args.timeout, retry_interval=args.retry_interval)
    except Exception as e:
        print(f"ERROR: {e}", file=sys.stderr)
        sys.exit(1)

    print("Dumping initial AT-SPI accessibility tree...")
    initial_tree = dump_node(app_acc)

    print("\nInitial accessibility tree overview:")
    print_tree_summary(initial_tree, indent=0, max_depth=3)

    modal_tree = None
    if not args.no_modal:
        print("\nTriggering modal dialog...")
        modal_triggered = trigger_modal(app_acc)
        # Allow egui frame loop to render modal dialog
        time.sleep(1.0)
        print("Dumping AT-SPI tree after modal trigger...")
        modal_tree = dump_node(app_acc)

        print("\nPost-modal accessibility tree overview:")
        print_tree_summary(modal_tree, indent=0, max_depth=3)

    os.makedirs(os.path.dirname(os.path.abspath(out_path)), exist_ok=True)

    if args.single_tree:
        payload = initial_tree
    else:
        payload = {
            "app_name": app_acc.get_name() or "gcu-fixture",
            "timestamp": time.time(),
            "tree": initial_tree,
            "modal_tree": modal_tree,
        }

    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(payload, f, ensure_ascii=False, indent=2)

    print(f"\nSuccessfully dumped AT-SPI tree to {out_path} ({os.path.getsize(out_path)} bytes)")


if __name__ == "__main__":
    main()
