# App Content

A view is plain data, but the entity of a node that is on screen is an ordinary Bevy entity, and
an app often wants more on it than the engine knows about: a marker component, an observer for
`Pointer<Click>`, a sound that plays when it is added. `ViewNode::with_scene` attaches a Bevy
scene to a node for exactly that.

```rust,ignore
ViewNode::sphere("door", 0.5).with_scene(|| {
    bsn! {
        Door
        on(|event: On<Pointer<Click>>, mut commands: Commands| {
            commands.trigger(DoorClicked(event.entity));
        })
    }
})
```

## The contract

- **The view stays pure.** `view(&S)` never reads or writes the world. The function you pass to
  `with_scene` is only data until the engine calls it, and the engine calls it only for a node
  that is shown.
- **Previews never build content.** While a drag evaluates states that are never shown, the
  engine reads the plain data of the nodes and calls no scene function. App components, hooks
  and observers therefore never run for a preview. To react to a preview, use `react_to`,
  `during` or `on_drop`.
- **The engine owns what it writes.** That is the name, the transform, the mesh, the material,
  the render layers, the picking and the visibility of the entity. The engine writes them after
  the scene, so the scene cannot change them.
- **The app owns the rest.** The scene is applied once, when the node first appears, and the
  engine never touches what it added again. Applying a scene twice would reset the state of the
  app, which is why it is applied once.
- **A node keeps its entity** for as long as its id is in the view, so components and observers
  survive every change of state. A node that disappears is despawned after its presence
  animation.
- **No children.** The engine owns the hierarchy of the nodes, so a scene must not describe
  children.
- **No opt-out.** There is no way for a view function to touch the world and no mode that
  builds content for previews.

A node that first appears while one view blends into another takes its scene from the view the
blend settles on.

## View functions are `Clone`

The engine keeps one copy of the view function for the model and one for each drag, and never
shares them. A closure that captures only `Copy` or `Clone` data, as most do, is `Clone` already.
A view function that captures something that is not, such as a `Box<dyn Fn>`, needs that
captured value to be wrapped in something that is.
