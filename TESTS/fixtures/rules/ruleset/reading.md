# Reading

Rules without a check: somebody has to read the code. Most rules of a real ruleset are of this kind.

## Names

#### `M-01` — a module is named after what it does

```rule
id = "M-01",
severity = "recommended",
```

Read the module and decide.

#### `M-02` — a boolean is named as a question

```rule
id = "M-02",
severity = "nice-to-have",
```

Read the code and decide.

## Errors

#### `M-03` — an error names the thing that failed, and a long title like this one has to wrap instead of pushing the result columns out of the window

```rule
id = "M-03",
severity = "recommended",
```

Read the messages and decide.

#### `N-01` — nothing is logged that should not be

```rule
id = "N-01",
severity = "critical",
```

Read the log calls and decide.
