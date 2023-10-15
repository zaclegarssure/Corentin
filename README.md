<div align="center">

# Corentin


An experimental reactive coroutine plugin for [`Bevy`](https://github.com/bevyengine/bevy).
</div>

```rust
async fn on_hit(mut scope: Scope, hp: Rd<Hp>, on_hp_change: OnChange<Hp>) {
  loop {
    let prev = hp.get(&scope);
    on_hp_change.observe(&mut scope).await;

    let curr = hp.get(&scope);
    if curr < prev {
        println!("Lost {} hp(s)", prev - curr);
    }
  }
}
```

# Objectives
This crate aims to provide a nice and lightweight implementation of reactive
coroutines for Bevy. Those are useful for scripting like logic, chain of
actions over time, complex scheduling and so on.

# Warning
This crate is under heavy development, none of the APIs are stable yet.

# Overview
* Define coroutines as async functions.
* Coroutines can yield and be resumed after a certain condition:
  This includes, waiting for some time, waiting until a component is mutated and so on.
* Coroutines are structured: they can spawn sub-coroutines, and wait until all of them
  terminates (`all`) or until one of them terminates (`first`). This also includes
  automatic cancellation.

## Example
TODO

## Features to implement soon
 * Using `Commands` to queue structural mutations and await them.
 * More forms of inter-coroutine communication, using `Signals`, `Producers` and `Receivers`.
 * The ability to run systems from coroutines, useful to define complex schedules.
 * More coroutine parameters, such as resources, queries and bevy events.

## Multithreading
For now the executor runs on a single thread, but ultimatly it should be
possible to run coroutines in parallel when needed.

# Contributions
It's a bit early to accept contributions right now, but if you're interested, don't hesitate to play around with this crate and share your ideas.

# License
All code in `corentin` is licensed under either:

- Apache License 2.0
- MIT License

at your option.
