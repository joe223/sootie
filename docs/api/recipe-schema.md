# Recipe Schema

Sootie recipes are JSON documents that can be saved with
`recipe_save`, listed with `recipes`, inspected with
`recipe_show`, deleted with `recipe_delete`, and executed with
`run`.

`recipe_save.recipe_json` accepts either this JSON object directly or a
string containing the same JSON.

## Top-Level Shape

```json
{
  "schema_version": 1,
  "name": "open-search",
  "description": "Open a browser search field and type a query.",
  "app": "Google Chrome",
  "params": {
    "query": {
      "type": "string",
      "description": "Text to search for.",
      "required": true
    }
  },
  "preconditions": {
    "app_running": "Google Chrome",
    "url_contains": "https://"
  },
  "steps": [],
  "on_failure": "stop"
}
```

| Field | Required | Meaning |
| --- | --- | --- |
| `schema_version` | yes | Positive integer schema version. |
| `name` | yes | Recipe name. It must not contain path separators. |
| `description` | no | Human description. |
| `app` | no | Default app selector applied to steps that do not provide one. |
| `params` | no | Named runtime parameters accepted by `run.params`. |
| `preconditions` | no | Conditions checked before steps run. |
| `steps` | no | Ordered recipe steps. |
| `on_failure` | no | Recipe-level failure policy. `skip` continues after a failed step; the default stops. |

`params` may be an object, `null`, or an empty array. Empty and null values are
treated as an empty parameter map for compatibility with older recordings.

## Parameter Substitution

Runtime params are passed through `run`:

```json
{
  "recipe": "open-search",
  "params": {
    "query": "Sootie"
  }
}
```

String values in step arguments can reference params with `{{name}}`:

```json
{
  "tool": "type",
  "args": {
    "text": "{{query}}"
  }
}
```

Required params declared with `"required": true` must be present in
`run.params`.

## Preconditions

```json
{
  "preconditions": {
    "app_running": "Google Chrome",
    "url_contains": "https://example.com"
  }
}
```

`app_running` verifies that the platform state includes the app. `url_contains`
checks the current context URL for the recipe app.

## Step Shapes

A step can call a Sootie tool directly:

```json
{
  "id": 1,
  "tool": "click",
  "args": {
    "query": "Search"
  },
  "wait_after": {
    "condition": "element_exists",
    "target": {
      "name": "Search"
    },
    "timeout_ms": 1000
  }
}
```

Or it can use a compatible action name that Sootie maps to a tool:

```json
{
  "id": 2,
  "action": "type",
  "target": {
    "role": "textfield",
    "computedNameContains": "Search"
  },
  "text": "{{query}}",
  "clear_first": true
}
```

| Step field | Meaning |
| --- | --- |
| `id` | Optional numeric step id. |
| `tool` | Direct Sootie tool name. If present, `args` is dispatched as-is. |
| `action` | Compatible action alias mapped to a Sootie tool. |
| `app` | Step app selector. |
| `args` | Direct tool arguments for `tool` steps. |
| `params` | Action step argument map. |
| `target` | Source selector or element target. |
| `to_target` | Destination selector for drag steps. |
| `text`, `key`, `keys`, `direction`, `amount`, `button`, `count`, `timeout`, `clear_first` | Compatible scalar aliases used by action steps. `set_clipboard` uses `text` as the clipboard payload. |
| `wait_after` | Optional wait condition after the step completes. |
| `note` | Human note. |
| `on_failure` | Step-level failure policy. `skip` continues after this step fails. |

## Target Shape

```json
{
  "app": "Google Chrome",
  "window": "Checkout",
  "coordinate": {
    "x": 100,
    "y": 200
  },
  "window_coordinate": {
    "x": 40,
    "y": 90
  },
  "window_normalized_coordinate": {
    "x": 0.5,
    "y": 0.5
  },
  "role": "button",
  "name": "Submit",
  "text": "Submit",
  "identifier": "submit-button",
  "dom_id": "submit",
  "dom_class": "primary",
  "computedNameContains": "Submit",
  "criteria": [
    {
      "attribute": "role",
      "value": "button",
      "matchType": "equals"
    }
  ]
}
```

Sootie maps `name`, `text`, `computedNameContains`, and criteria values into
the selector fields used by MCP tools. `coordinate` maps to screen `x` and `y`.
`window_coordinate` maps to a point relative to the current app window.
`window_normalized_coordinate` maps to a point inside the current app window
where `0.0, 0.0` is the top-left corner and `1.0, 1.0` is the bottom-right
corner. `normalized_coordinate` is accepted as an alias for
`window_normalized_coordinate`.

When a target contains both semantic selectors and a coordinate, `run`
uses the semantic selector first. The coordinate is retained as a fallback and
is remapped against the current app/window frame when the semantic target cannot
be resolved. Durable recipes should therefore prefer `name`, `role`, `dom_id`,
or other semantic fields, with `window_normalized_coordinate` as the coordinate
fallback for resized or moved windows.
If `window` is provided for a coordinate target, that window title must match
the current app state. Sootie will not remap coordinates against a different
focused or first window when the explicit window is missing.

`learn_stop` returns both the raw recorded `actions` and a generated
`recipe`/`recipe_json` draft. The generated recipe follows the same rule:
semantic information is preserved when available, and pointer coordinates are
stored as window-normalized fallbacks when Sootie can identify the active window
frame. When learning mode records a successful paste hotkey and the current
clipboard contains text, the raw action includes `clipboard_text` and the
generated recipe inserts a recipe-scoped `set_clipboard` step immediately before
the paste. This makes learned text/SVG paste workflows reproducible without
adding a public clipboard MCP tool.

## Action Mapping

| Recipe action | Sootie tool |
| --- | --- |
| `screenshot` | `screenshot` |
| `click` | `click` |
| `hover` | `hover` |
| `long_press` | `long_press` |
| `drag` | `drag` |
| `type` | `type` |
| `paste_text` | `type` |
| `set_clipboard` / `clipboard_set` | Internal recipe clipboard step |
| `press` | `press` |
| `hotkey` | `hotkey` |
| `scroll` | `scroll` |
| `focus` | `focus` |
| `window` | `window` |
| `wait` | `wait` or a delay step |

`set_clipboard` is intentionally recipe-scoped rather than a public MCP tool. It
lets a durable recipe stage text, SVG, or other text-based paste payloads before
a normal keyboard paste action:

```json
{
  "id": 1,
  "action": "set_clipboard",
  "text": "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"
}
```

## Wait Conditions

```json
{
  "condition": "element_exists",
  "target": {
    "computedNameContains": "Done"
  },
  "timeout": 2,
  "interval_ms": 100
}
```

`timeout` and `interval` are seconds. `timeout_ms` and `interval_ms` are
millisecond aliases. A wait condition of `delay` creates a fixed delay step.

## Storage

Recipes are stored as pretty JSON files in the platform data directory under
`sootie/recipes`. Recipe names cannot contain `/`, `\`, or `..`.

## Minimal Example

```json
{
  "schema_version": 1,
  "name": "type-message",
  "app": "TextEdit",
  "params": {
    "message": {
      "type": "string",
      "required": true
    }
  },
  "steps": [
    {
      "action": "type",
      "text": "{{message}}"
    }
  ]
}
```
