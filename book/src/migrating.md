# Migrating from 0.2 to 0.3

Version 0.3 removes the last shared ownership from the library. Three things change for code
that uses it, and one thing is new.

## View functions must be `Clone`

`ViewFn<S>` was an `Arc<dyn Fn(&S) -> View<S>>`. It is now a struct that owns a boxed closure
and clones whatever the closure captures, so the model, the plugin and every drag each hold
their own copy instead of sharing one.

- `DraggablePlugin::new`, `DragModel::new` and `DragContext::new` take
  `impl Fn(&S) -> View<S> + Clone + Send + Sync + 'static`. A closure that captures nothing, or
  only `Copy` and `Clone` data, already is `Clone`, so most code compiles unchanged.
- A closure that captures something that is not `Clone` needs it wrapped in something that is,
  or the data needs to move into the state `S`, where a view function belongs anyway.
- Code that built an `Arc` to pass as a `ViewFn` passes the closure itself. Code that called a
  `ViewFn` as a function calls `ViewFn::call`. `DragContext::with_view_fn` takes a `ViewFn`
  that is already built.

## Heightmaps are flat buffers

`Heightmap` no longer shares chunks through `Arc`. It keeps the heights and the holes of the
whole map in two flat buffers laid out chunk by chunk, so copying one is two memory copies and
an edit still reports the chunks it reaches through `chunk_key` and `chunk_heights`. The public
methods are unchanged; only code that relied on a copy being a few pointer clones notices.

## The `bevy_scene` feature

The crate now depends on Bevy's `bevy_scene` feature, and `DraggablePlugin` adds `ScenePlugin`
when the app has none.

## New: scenes on nodes

`ViewNode::with_scene` gives a node components and observers of the app's own. The scene is
built only when the node first appears, never for the previews of a drag. See
[App Content](./app-content.md) for the contract.
