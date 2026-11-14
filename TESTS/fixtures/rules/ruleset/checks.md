# Checks

The rules of the Rules tab's test sheet (`TESTS/sheets/rules-tab.sheet.lua`) that the engine can answer by itself.

## Mechanical

#### `Q-01` — a word that must not appear

```rule
id = "Q-01",
severity = "recommended",
check = { type = "grep", pattern = "FORBIDDEN", include = "%.lua$" },
```

`lua/a.lua` of the fixture project holds the word, so this rule fails with a finding.

#### `Q-02` — a word nobody wrote

```rule
id = "Q-02",
severity = "recommended",
check = { type = "grep", pattern = "NEVER_WRITTEN_ANYWHERE", include = "%.lua$" },
```

Passes.

## Code

#### `P-01` — a predicate

```rule
id = "P-01",
severity = "recommended",
check = {
  type = "lua_predicate",
  fn = function(root)
    return true
  end,
},
```

Code from the ruleset: it runs only once the ruleset is trusted, and reports `predicate not trusted` until then.
