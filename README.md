*This library is now deprecated. Please consider using an alternative, such as [tokio::sync::Mutex](https://docs.rs/tokio/latest/tokio/sync/struct.Mutex.html).*

# wasm_mutex
 
`wasm_mutex::Mutex` is specifically used for single-threaded WebAssembly applications. Similar to `std::sync::Mutex`, the data can be accessed through `lock` or `try_lock`, which guarantees that the data is only ever accessed when the mutex is locked.

Data stored in a `RefCell<T>` encounter a `BorrowError` or `BorrowMutError` when multiple closures attempt to access the same data simulatenously (e.g. a click event handler and a set interval handler). Similarly, a `std::sync::Mutex` will panic under the same circumstances, due to calling lock while the lock is already being held by the current thread. `wasm_mutex::Mutex` will allow the data to be locked and unlocked, allowing single access to the data at any given time.

## Example Usage

```rust
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_mutex::Mutex;
use wasm_bindgen_futures::spawn_local;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
    #[wasm_bindgen(js_name = setInterval)]
    fn set_interval(closure: &Closure<dyn FnMut()>, millis: u32) -> i32;
}

#[wasm_bindgen(start)]
pub async fn main() -> std::result::Result<(), JsValue> {
    let count = Rc::new(Mutex::new(0));

    {
        let locked_count = count.lock().await;
        log(&format!("Starting count: {}", *locked_count));
    }

    let c = count.clone();
    let increment = Closure::wrap(Box::new(move || {
        let c = c.clone();
        spawn_local(async move {
            // wait for lock to be released before incrementing value
            let mut locked_count = c.lock().await;
            *locked_count += 1;
        });
    }) as Box<dyn FnMut()>);

    let c = count.clone();
    let print_count = Closure::wrap(Box::new(move || {
        // if data is unlocked, lock and print current count value
        if let Some(locked_count) = c.try_lock() {
            log(&format!("Current count: {}", *locked_count));
        }
    }) as Box<dyn FnMut()>);

    set_interval(&increment, 1000);
    set_interval(&print_count, 3000);

    increment.forget();
    print_count.forget();
    
    Ok(())
}
```