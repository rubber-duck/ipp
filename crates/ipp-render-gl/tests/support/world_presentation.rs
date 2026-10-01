//! Release fixture World borrows before Host preparation and immutable drawing.

macro_rules! present_world {
    (finish; $function:expr, $renderer:expr, $host:ident, $world:ident, $($argument:expr),* $(,)?) => {{
        let id = $world.id();
        drop($world);
        $function($renderer, &mut $host, id, $($argument),*)
    }};

    ($function:expr, $renderer:expr, $host:ident, $world:ident, $($argument:expr),* $(,)?) => {{
        let id = $world.id();
        drop($world);
        let result = $function($renderer, &mut $host, id, $($argument),*);
        $world = $host.world_mut(id).unwrap();
        result
    }};
}

pub(crate) use present_world;
