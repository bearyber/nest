// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { useRef, useState } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useDialogFocus, type DialogFocusOptions } from "./useDialogFocus";

afterEach(cleanup);

function Dialog({ label, children, opts }: { label: string; children?: React.ReactNode; opts?: DialogFocusOptions }) {
  const ref = useRef<HTMLDivElement>(null);
  useDialogFocus(ref, opts);
  return (
    <div ref={ref} role="dialog" aria-label={label} tabIndex={-1}>
      {children}
    </div>
  );
}

function App() {
  const [outer, setOuter] = useState(false);
  const [inner, setInner] = useState(false);
  return (
    <div>
      <div id="app">
        <button onClick={() => setOuter(true)}>Open</button>
        <button>Other</button>
      </div>
      {outer && (
        <Dialog label="Outer">
          <input aria-label="Name" />
          <button onClick={() => setInner(true)}>More</button>
          {inner && (
            <Dialog label="Inner">
              <button>One</button>
              <button onClick={() => setInner(false)}>Done</button>
            </Dialog>
          )}
          <button onClick={() => setOuter(false)}>Close</button>
        </Dialog>
      )}
    </div>
  );
}

const tab = (shift = false) => fireEvent.keyDown(document.activeElement!, { key: "Tab", shiftKey: shift });

describe("useDialogFocus", () => {
  it("moves focus in, traps Tab, makes the rest inert, and gives focus back", () => {
    render(<App />);
    const opener = screen.getByRole("button", { name: "Open" });
    opener.focus();
    fireEvent.click(opener);

    // Focus in: the first field.
    expect(document.activeElement).toBe(screen.getByLabelText("Name"));
    // The rest of the app is inert; the dialog isn't.
    expect(document.getElementById("app")!.hasAttribute("inert")).toBe(true);
    expect(screen.getByRole("dialog", { name: "Outer" }).closest("[inert]")).toBeNull();

    // Trap: Shift+Tab from the first goes to the last, Tab from the last back to the first.
    tab(true);
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Close" }));
    tab();
    expect(document.activeElement).toBe(screen.getByLabelText("Name"));

    // Close: focus goes back to the opener and nothing stays inert.
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(document.activeElement).toBe(opener);
    expect(document.querySelector("[inert]")).toBeNull();
  });

  it("works for a dialog inside a dialog", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    const more = screen.getByRole("button", { name: "More" });
    more.focus();
    fireEvent.click(more);

    // No text field: the first button.
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "One" }));
    // The outer dialog's other controls are inert; the inner one isn't.
    expect(screen.getByLabelText("Name").closest("[inert]")).not.toBeNull();
    expect(screen.getByRole("dialog", { name: "Inner" }).closest("[inert]")).toBeNull();
    // Tab stays in the inner dialog.
    screen.getByRole("button", { name: "Done" }).focus();
    tab();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "One" }));

    // Closing the inner one: back to "More"; the outer is usable, the app behind it still inert.
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(document.activeElement).toBe(more);
    expect(screen.getByLabelText("Name").closest("[inert]")).toBeNull();
    expect(document.getElementById("app")!.hasAttribute("inert")).toBe(true);
  });

  it("non-modal: focus moves in and back, but nothing is trapped or inert", () => {
    function Panel() {
      const [open, setOpen] = useState(false);
      return (
        <div>
          <button onClick={() => setOpen((v) => !v)}>Toggle</button>
          {open && (
            <Dialog label="Panel" opts={{ modal: false, initialFocus: "container" }}>
              <button onClick={() => setOpen(false)}>Shut</button>
            </Dialog>
          )}
        </div>
      );
    }
    render(<Panel />);
    const toggle = screen.getByRole("button", { name: "Toggle" });
    toggle.focus();
    fireEvent.click(toggle);
    expect(document.activeElement).toBe(screen.getByRole("dialog", { name: "Panel" }));
    expect(document.querySelector("[inert]")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Shut" }));
    expect(document.activeElement).toBe(toggle);
  });

  it("focuses a field that appears after opening (loaded content)", async () => {
    const Loading = ({ ready }: { ready: boolean }) => (
      <Dialog label="Loading">{ready && <input aria-label="Late" />}</Dialog>
    );
    const { rerender } = render(<Loading ready={false} />);
    expect(document.activeElement).toBe(screen.getByRole("dialog"));
    rerender(<Loading ready />);
    await act(async () => {}); // MutationObserver callbacks run as a microtask
    expect(document.activeElement).toBe(screen.getByLabelText("Late"));
  });

  it("a second dialog opened beside the first (menu bar shortcut) stays usable", async () => {
    function Two({ second }: { second: boolean }) {
      return (
        <div>
          <Dialog label="First">
            <input aria-label="A" />
          </Dialog>
          {second && (
            <Dialog label="Second">
              <input aria-label="B" />
            </Dialog>
          )}
        </div>
      );
    }
    const { rerender } = render(<Two second={false} />);
    rerender(<Two second />);
    await act(async () => {}); // let the first dialog's observer see the new sibling
    expect(screen.getByRole("dialog", { name: "Second" }).closest("[inert]")).toBeNull();
    expect(screen.getByRole("dialog", { name: "First" }).hasAttribute("inert")).toBe(true);
    expect(document.activeElement).toBe(screen.getByLabelText("B"));
  });
});
