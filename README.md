<div align="center">

# Corentin


An experimental reactive coroutine plugin for [`Bevy`](https://github.com/bevyengine/bevy).
</div>

```rust
async fn count(mut fib: Fib) {
  loop {
    fib.duration(Duration::from_secs(1)).await;
    i += 1;
    println!("This coroutine has started since {} seconds", i);
  }
}
```

# Objectives
This crate aims to provide a nice and lightweight implementation of reactive coroutines
for Bevy. Those would be useful for scripting like logic, chain of actions over time, and so on.

# Warning
Most features are not implemented yet and those that are available are pretty slow (and probably buggy as well).

# Overview

## What's already done ?
 * Spawning coroutines in the world, or owned by an entity.
 * Waiting for a certain amount of time.
 * Waiting for the next tick.
 * Waiting that a component from an entity change.
 * Spawning multiple coroutine and waiting for the first one to finish.

## What will be soon available
 * Make this an actual bevy plugin.
 * Spawning multiple coroutine and waiting for all of them to finish.
 * Getting immutable access to specific components from an entity.
 * Getting mutable access to specific components from an entity.
 * "Binding" a coroutine to some components of an entity, so that those can be conveniently accessed in the entire coroutine.

## What will be done way later
 * Optimize all of that, in particular to reduce heap allocations of coroutines.
 * The possibility to run arbitrary queries from a coroutine.
 * Higher level construct and macros to make everything simpler to write.
 * A multithreaded runtime (no idea how I will make that one work)
 * A way to serialize and deserialize coroutine state.

TODO: add examples of what is currently possible, and what api will be available later on as well.

# Contributions
It's a bit early to accept contributions right now, but if you're interested, don't hesitate to play around with this crate and share your ideas.
