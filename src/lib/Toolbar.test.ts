import { expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import Toolbar from "./Toolbar.svelte";

function renderToolbar(props: Record<string, unknown> = {}) {
  return render(Toolbar, {
    props: {
      sidebarCollapsed: false,
      sidebarWidth: 230,
      onToggleSidebar: vi.fn(),
      onRefresh: vi.fn(),
      onCompose: vi.fn(),
      onSearch: vi.fn(),
      onOpenSettings: vi.fn(),
      ...props,
    },
  });
}

it("fires the chrome callbacks from their buttons", async () => {
  const onToggleSidebar = vi.fn();
  const onRefresh = vi.fn();
  const onCompose = vi.fn();
  const onOpenSettings = vi.fn();
  renderToolbar({ onToggleSidebar, onRefresh, onCompose, onOpenSettings });

  await fireEvent.click(screen.getByRole("button", { name: "Toggle sidebar" }));
  expect(onToggleSidebar).toHaveBeenCalledOnce();

  await fireEvent.click(
    screen.getByRole("button", { name: "Check for new mail" }),
  );
  expect(onRefresh).toHaveBeenCalledOnce();

  await fireEvent.click(screen.getByRole("button", { name: "New Message" }));
  expect(onCompose).toHaveBeenCalledOnce();

  await fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(onOpenSettings).toHaveBeenCalledOnce();
});

it("reports what is typed into the search field", async () => {
  const onSearch = vi.fn();
  renderToolbar({ onSearch });

  const input = screen.getByRole("searchbox", { name: "Search messages" });
  await fireEvent.input(input, { target: { value: "from:alice" } });

  expect(onSearch).toHaveBeenCalledWith("from:alice");
});

it("sizes the traffic-light zone to the sidebar and shrinks it collapsed", async () => {
  const { container, rerender } = renderToolbar({ sidebarWidth: 260 });

  const zone = container.querySelector<HTMLElement>(".sidebar-zone");
  expect(zone?.style.width).toBe("260px");

  await rerender({ sidebarCollapsed: true, sidebarWidth: 260 });
  expect(zone?.style.width).toBe("130px");
});
