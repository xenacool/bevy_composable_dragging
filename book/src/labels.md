# Text Labels

Text labels provide a way to add readable names to draggable nodes. The [`LabelPlugin`](label/struct.LabelPlugin.html) keeps `Text2d` entities in sync with `ViewNode` positions, so labels follow their nodes when dragged.

## Setup

Add the plugin after inserting a `LabelsResource` that describes the labels for each state:

```rust
use bevy_composable_dragging::label::{Label, LabelPlugin, LabelsResource};

#[derive(Clone)]
struct Editor {
    nodes: Vec<(u32, String)>,
}

fn labels(state: &Editor, _view: &View<Editor>) -> Vec<Label> {
    state.nodes.iter()
        .map(|(id, name)| Label::new(format!("label-{id}"), name.clone(), format!("node-{id}")))
        .collect()
}

app.insert_resource(LabelsResource::<Editor>::new(labels, 3))
   .add_plugins(LabelPlugin::<Editor>::default());
```

The second argument to `LabelsResource::new` is the render layer for the labels. Choose a layer that no scene camera renders, otherwise the text would be drawn twice at the wrong size.

## How it works

Every frame the plugin projects each label's parent node through the scene camera to a screen pixel and moves the text there. The label camera uses one world unit per logical pixel with the origin at the center of the window, so text has the exact pixel size requested regardless of window size or camera scaling mode.

This projection works with flat, isometric, and perspective cameras. A label whose node is behind the camera or outside its depth range is automatically hidden.

## Label API

Each `Label` has:

- `key`: a stable identifier so the plugin reuses the same entity across frames.
- `text`: the string to display.
- `parent`: the `ViewNode` id the label follows. When `None`, the label sits at a fixed world position given by `at`.
- `offset`: pixel offset from the projected position, with Y pointing up. The default centers the text on the node.
- `color`: text color, defaulting to white.
- `font_size`: font size in pixels, defaulting to 14.
- `layer`: render layer, defaulting to 0.

```rust
Label::new("label-1", "Start", "node-1")
    .with_offset(Vec2::new(0.0, 20.0))
    .with_color(Color::WHITE)
    .with_font_size(16.0)
```

### Fixed position labels

Labels without a parent sit at a fixed world position:

```rust
Label::at("fps", "60 FPS", Vec3::new(-200.0, 200.0, 0.0))
```

### Label offsets

The `offset` field moves the text in pixels from the projected node position. Positive Y moves the text up, positive X moves it right. Use this to place labels above, below, or beside nodes:

```rust
// Label above the node
Label::new("label-1", "Node A", "node-1")
    .with_offset(Vec2::new(0.0, 20.0))

// Label below the node
Label::new("label-1", "Node A", "node-1")
    .with_offset(Vec2::new(0.0, -20.0))
```

## Helper: labels_from_view

The `labels_from_view` function builds labels for every node in a view:

```rust
use bevy_composable_dragging::label::labels_from_view;

let labels = labels_from_view(view, &|id| {
    Some(format!("Node {id}"))
});
```

## Example

The `dag` example demonstrates labels on a directed acyclic graph. Each node shows a letter label (A-E) that follows the node when dragged.
