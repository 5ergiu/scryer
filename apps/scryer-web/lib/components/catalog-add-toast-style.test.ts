import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const card = readFileSync(
  new URL("../../components/root/catalog-add-toast.tsx", import.meta.url),
  "utf8",
);
const toaster = readFileSync(
  new URL("../../components/ui/sonner.tsx", import.meta.url),
  "utf8",
);

test("catalog activity cards retain the standard success border above global CSS", () => {
  const successBorder = "var(--scry-success-border)";
  assert.ok(toaster.includes(`"--success-border": "${successBorder}"`));
  // The unlayered global * border-color rule overrides ordinary Tailwind utilities.
  assert.ok(card.includes(`!border-[${successBorder}]`));
  assert.ok(card.includes("!border-0"), "the outer Sonner frame stays borderless");
});

test("the warning toast paints an opaque surface, like success and error", () => {
  // `!bg-[var(--card)]` is the mechanism: Sonner's richColors sets a translucent
  // `--warning-bg`, and only an important utility beats it. Warning used to
  // reach for `bg-[linear-gradient(...),var(--scry-bg)]`, which Tailwind emits
  // as `background-image` — a bare colour is not a valid layer there, so the
  // browser dropped the whole declaration and the translucent default showed.
  for (const variant of ["success", "error", "warning"]) {
    assert.ok(
      new RegExp(`${variant}:\\s*\\n?\\s*"[^"]*!bg-\\[var\\(--card\\)\\]`).test(
        toaster,
      ),
      `${variant} toast must force the opaque card surface`,
    );
  }
  assert.ok(
    !toaster.includes("var(--scry-warning-bg)),var(--scry-bg)"),
    "the invalid background-image layering must not come back",
  );
});
