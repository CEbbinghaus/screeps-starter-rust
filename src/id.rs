use std::cell::RefCell;

use uuid::{Builder, Bytes, Uuid};
use rand::{SeedableRng, RngCore};
use rand::rngs::{StdRng};

thread_local! {
    static RNG: RefCell<StdRng> = RefCell::from(StdRng::seed_from_u64(js_sys::Math::random().to_bits()));
}

pub fn get_id() -> Uuid {
    let mut bytes: Bytes = [0; 16];

    RNG.with(|f| {
        f.borrow_mut().try_fill_bytes(&mut bytes).expect("Filling bytes to work");
    });

    return Builder::from_random_bytes(bytes).into_uuid();
}