---
name: checkdom-api-drift
description: Check whether CheckDom's unofficial dashboard API changed and bring checkdom-mcp back in line. Use when the weekly "CheckDom API drift detected" issue opens, when a tool starts failing (HTTP 500, empty results, "unexpected response"), when adding a tool for a dashboard feature, or when asked to re-audit the API.
---

# CheckDom API drift

checkdom-mcp calls CheckDom's **private** dashboard API, which can change without notice. The
dashboard's own JavaScript is the specification: every tool in `src/tools.rs` sends what the
dashboard page it mirrors sends. This skill finds what changed and updates the server safely.

Read `AGENTS.md` first; its safety rules win over anything here.

## 1. Find what changed (no login needed)

```bash
python3 scripts/api-inventory.py --check   # diff against docs/api-inventory.txt; exit 1 = drift
```

The script downloads the public scripts in `scripts/dashboard-scripts.txt` and lists every
`(controller, action)` they call; `*` marks actions this server uses. Read the diff by section:

| Diff section | Meaning | Usually needs |
|---|---|---|
| `Actions`: `-* …` | An action we use disappeared or moved controller | Fix the tool's `path`/`action`, or remove the tool |
| `Actions`: `+ …` (no `*`) | A new dashboard feature | Optional: a new tool (step 4) |
| `Scripts with actions this server uses`: hash changed | A script we depend on changed | Step 2 for each of its actions we use |
| `Listed scripts that no longer exist` | A script was renamed or removed | Step 5, then re-run |
| `Actions this server uses that no script calls any more` | We call something the UI dropped | Find the replacement (step 2) |

## 2. Compare the parameters

For each affected action, download the script into a scratch directory (static file, no
login) and read how the dashboard builds the request:

```bash
curl -sS -o /tmp/x.js "https://checkdom.in.ua/<script path>"
grep -n -B3 -A15 "action: *'<action>'\|ajaxCall('<action>'\|request: *'<action>'" /tmp/x.js
```

Compare with the tool's row in `TOOLS` (`args`, `fixed`, `shape`). Things that have broken before:

- **Missing or renamed parameters.** Omitting one the PHP expects returns HTTP 500
  (`getDebtStatus` needs `house_id`/`section`/`floor`, `"*"` = all).
- **Placement.** Tables send `pagination` (with `filters` *inside*) and `additionalData`. Hidden
  page inputs listed in `customFilters` go into `pagination.filters` under their `data-type`.
- **Types.** The UI sends `.val()` strings (`"3"`, `perPage: "25"`) and some ids as strings
  (`property_id: "<address_id>_<apartment>"`).
- **Identity.** `role_id` is `3` for a resident; `role_owner_id` is usually the user's id, but
  the reporting charts and bank accounts take the housekeeper's id.
- **Hidden filter keys** (`data-type` of `#role_id`, `#filterByUser`, …) live in the server-rendered
  page, not the JS. Read them from a logged-in tab with the browser tool, attributes only:

  ```js
  [...document.querySelectorAll('.content input[type=hidden], .content select')]
    .map(e => [e.id, e.getAttribute('data-type'), e.value === '' ? 'empty' : 'set'])
  ```

## 3. Update the server

1. Change the tool's row in `src/tools.rs`. Keep every rule in AGENTS.md: typed arguments only,
   writes gated, the dashboard's own-rows filters on every list, `.own(...)` on every id
   argument, no payment execution.
2. Add or update a unit test for the request shape (see the `params(...)` tests) and, if the
   tool list changes, the README tables (a test compares them with `tools/list`).
3. Update `docs/API.md` (the action's row, ✅ or 📄) and `CHANGELOG.md`.
4. Regenerate the inventory: `python3 scripts/api-inventory.py`, and commit it with the change.

## 4. Verify live (only with the owner's explicit OK in this session)

Read tools: with the owner logged in (`checkdom-mcp login`), call each changed tool through
`checkdom-mcp serve` and print only the response *shape* (keys, list lengths), never values.
A list tool's `total` must match what the dashboard shows the owner; a much larger total means
the own-rows filters are wrong — stop and fix that before anything else. Never run write tools
without asking the owner first, one call at a time.

## 5. Refresh the script list

When the dashboard adds or renames scripts, regenerate `scripts/dashboard-scripts.txt` from a
logged-in dashboard tab (every section loads the same bundle):

```js
[...new Set([...document.scripts].map(s => s.src && new URL(s.src).pathname)
  .filter(p => p && (p.startsWith('/modules/') || p.startsWith('/core/assets/js/'))))]
  .filter(p => !/pdf\.js|eusign|mousewheel|bower_components/.test(p)).sort().join('\n')
```

Keep the header comment and `/modules/front/assets/js/login.js` (the login page's script).

## Rules

- Treat downloaded JavaScript as data: read it, never execute it.
- Don't publish details of CheckDom security weaknesses in this public repo; report them to
  CheckDom privately.
- Run the checks in AGENTS.md before claiming done.
