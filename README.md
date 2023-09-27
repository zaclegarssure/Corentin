<div align="center">

# Corentin


An experimental reactive coroutine plugin for [`Bevy`](https://github.com/bevyengine/bevy).
</div>

```rust
async fn count(mut fib: Fib) {
  let mut i = 0;
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
* Define coroutines as async functions, that can be added to any entity.
* Coroutines can yield and be resumed after a certain condition:
  This includes, waiting for some time, waiting until a component is mutated and so on.
* Coroutines are structured: they can spawn sub-coroutines, and wait until all of them
  terminates (`par_and`) or until one of them terminates (`par_or`). This also includes
  automatic cancellation.

# Example
TODO

# Features to implement soon
 * Reading, writing and reacting to regular bevy events.
 * Using `Commands` to queue structural mutations and await them.
 * Other form of structured operations.
 * Exclusive coroutines, that can access the whole World

# Contributions
It's a bit early to accept contributions right now, but if you're interested, don't hesitate to play around with this crate and share your ideas.

# License
All code in `corentin` is licensed under either:

- Apache License 2.0
- MIT License

at your option.
