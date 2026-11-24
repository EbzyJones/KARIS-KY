# Frontend accessibility baseline

The current frontend is a single static status page in `index.html`. The
automated baseline is zero axe-core violations for WCAG 2.0 A/AA, WCAG 2.1
A/AA, and WCAG 2.2 AA rules in the loading, backend-available, and
backend-unavailable states.

Run the check:

```sh
npm ci --prefix docs
npm run test:a11y --prefix docs
```

The live backend message is exposed as a polite status announcement. The green
page badge uses dark text to keep its text contrast above the AA threshold.
There are currently no interactive controls that require keyboard focus.

This is an automated baseline, not a full WCAG audit. Axe's color-contrast
rule is incomplete in jsdom because canvas rendering is unavailable; the page
badge's computed text and background colors are checked separately against
the 4.5:1 AA threshold. The test also cannot assess general visual layout,
real-browser rendering, or complete keyboard and screen-reader behavior. When
interactive or investor-specific views are added, include their rendered
states in the axe checks and manually verify keyboard operation, visible focus,
and contrast in a browser.
