# Recipe Schema

Sootie recipes are JSON documents that can be saved with
`sootie_recipe_save`, listed with `sootie_recipes`, inspected with
`sootie_recipe_show`, deleted with `sootie_recipe_delete`, and executed with
`sootie_run`.

`sootie_recipe_save.recipe_json` accepts either this JSON object directly or a
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
| `params` | no | Named runtime parameters accepted by `sootie_run.params`. |
| `preconditions` | no | Conditions checked before steps run. |
| `steps` | no | Ordered recipe steps. |
| `on_failure` | no | Recipe-level failure policy. `skip` continues after a failed step; the default stops. |

`params` may be an object, `null`, or an empty array. Empty and null values are
treated as an empty parameter map for compatibility with older recordings.

## Parameter Substitution

Runtime params are passed through `sootie_run`:

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
  "tool": "sootie_type",
  "args": {
    "text": "{{query}}"
  }
}
```

Required params declared with `"required": true` must be present in
`sootie_run.params`.

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
  "tool": "sootie_click",
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
| `text`, `key`, `keys`, `direction`, `amount`, `button`, `count`, `timeout`, `clear_first` | Compatible scalar aliases used by action steps. |
| `wait_after` | Optional wait condition after the step completes. |
| `note` | Human note. |
| `on_failure` | Step-level failure policy. `skip` continues after this step fails. |

## Target Shape

```json
{
  "app": "Google Chrome",
  "coordinate": {
    "x": 100,
    "y": 200
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
`window_coordinate` is accepted for compatibility and maps to the same point
shape.

## Action Mapping

| Recipe action | Sootie tool |
| --- | --- |
| `screenshot` | `sootie_screenshot` |
| `click` | `sootie_click` |
| `hover` | `sootie_hover` |
| `long_press` | `sootie_long_press` |
| `drag` | `sootie_drag` |
| `type` | `sootie_type` |
| `paste_text` | `sootie_type` |
| `press` | `sootie_press` |
| `hotkey` | `sootie_hotkey` |
| `scroll` | `sootie_scroll` |
| `focus` | `sootie_focus` |
| `window` | `sootie_window` |
| `wait` | `sootie_wait` or a delay step |

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
