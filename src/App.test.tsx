import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import App from "./App";

describe("App shell", () => {
  it("renders the three panes", () => {
    render(<App />);
    expect(screen.getByLabelText("Sidebar")).toBeTruthy();
    expect(screen.getByLabelText("Note list")).toBeTruthy();
    expect(screen.getByRole("main")).toBeTruthy();
  });
});
