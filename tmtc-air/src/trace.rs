#[unsafe(no_mangle)]
extern "Rust" fn _embassy_trace_poll_start(executor_id: u32) {
    defmt::trace!("executor_poll_start exec={=u32}", executor_id);
}

#[unsafe(no_mangle)]
extern "Rust" fn _embassy_trace_task_new(executor_id: u32, task_id: u32) {
    defmt::trace!(
        "task_new exec={=u32} task={=u32}",
        executor_id,
        task_id
    );
}

#[unsafe(no_mangle)]
extern "Rust" fn _embassy_trace_task_end(executor_id: u32, task_id: u32) {
    defmt::trace!(
        "task_end exec={=u32} task={=u32}",
        executor_id,
        task_id
    );
}

#[unsafe(no_mangle)]
extern "Rust" fn _embassy_trace_task_exec_begin(executor_id: u32, task_id: u32) {
    defmt::trace!(
        "task_exec_begin exec={=u32} task={=u32}",
        executor_id,
        task_id
    );
}

#[unsafe(no_mangle)]
extern "Rust" fn _embassy_trace_task_exec_end(executor_id: u32, task_id: u32) {
    defmt::trace!(
        "task_exec_end exec={=u32} task={=u32}",
        executor_id,
        task_id
    );
}

#[unsafe(no_mangle)]
extern "Rust" fn _embassy_trace_task_ready_begin(executor_id: u32, task_id: u32) {
    defmt::trace!(
        "task_ready_begin exec={=u32} task={=u32}",
        executor_id,
        task_id
    );
}

#[unsafe(no_mangle)]
extern "Rust" fn _embassy_trace_executor_idle(executor_id: u32) {
    defmt::trace!("executor_idle exec={=u32}", executor_id);
}
