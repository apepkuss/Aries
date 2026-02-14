(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit"
    (func $proc_exit (param i32)))

  (memory (export "memory") 1)

  ;; "Hello, WASM!\n" at memory offset 8
  (data (i32.const 8) "Hello, WASM!\n")

  ;; iov struct at offset 0: [ptr=8, len=13]
  (data (i32.const 0) "\08\00\00\00\0d\00\00\00")

  (func $main (export "_start")
    (call $fd_write
      (i32.const 1)
      (i32.const 0)
      (i32.const 1)
      (i32.const 24)
    )
    drop
    (call $proc_exit (i32.const 0))
  )
)
