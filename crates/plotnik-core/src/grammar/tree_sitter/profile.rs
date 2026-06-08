use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ProfileSnapshot {
    pub parse_states_new: u64,
    pub parse_states_reused: u64,
    pub transitive_closure_calls: u64,
    pub transitive_closure_input_entries: u64,
    pub transitive_closure_output_entries: u64,
    pub transitive_closure_max_input_entries: u64,
    pub transitive_closure_max_output_entries: u64,
    pub add_item_calls: u64,
    pub add_item_nonterminal_calls: u64,
    pub closure_additions_considered: u64,
    pub inline_sites: u64,
    pub inline_productions: u64,
    pub item_set_insert_calls: u64,
    pub item_set_insert_len_sum: u64,
    pub item_set_insert_max_len: u64,
    pub item_set_insert_appended: u64,
    pub item_set_insert_last_equal: u64,
    pub item_set_insert_binary_searches: u64,
    pub item_set_insert_binary_new: u64,
    pub item_set_insert_binary_existing: u64,
    pub item_set_insert_shifted_entries: u64,
    pub parse_item_cmp_calls: u64,
    pub parse_item_cmp_step_iterations: u64,
    pub parse_item_eq_calls: u64,
    pub parse_item_eq_step_iterations: u64,
    pub parse_item_hash_calls: u64,
    pub parse_item_hash_step_iterations: u64,
}

static PARSE_STATES_NEW: AtomicU64 = AtomicU64::new(0);
static PARSE_STATES_REUSED: AtomicU64 = AtomicU64::new(0);
static TRANSITIVE_CLOSURE_CALLS: AtomicU64 = AtomicU64::new(0);
static TRANSITIVE_CLOSURE_INPUT_ENTRIES: AtomicU64 = AtomicU64::new(0);
static TRANSITIVE_CLOSURE_OUTPUT_ENTRIES: AtomicU64 = AtomicU64::new(0);
static TRANSITIVE_CLOSURE_MAX_INPUT_ENTRIES: AtomicU64 = AtomicU64::new(0);
static TRANSITIVE_CLOSURE_MAX_OUTPUT_ENTRIES: AtomicU64 = AtomicU64::new(0);
static ADD_ITEM_CALLS: AtomicU64 = AtomicU64::new(0);
static ADD_ITEM_NONTERMINAL_CALLS: AtomicU64 = AtomicU64::new(0);
static CLOSURE_ADDITIONS_CONSIDERED: AtomicU64 = AtomicU64::new(0);
static INLINE_SITES: AtomicU64 = AtomicU64::new(0);
static INLINE_PRODUCTIONS: AtomicU64 = AtomicU64::new(0);
static ITEM_SET_INSERT_CALLS: AtomicU64 = AtomicU64::new(0);
static ITEM_SET_INSERT_LEN_SUM: AtomicU64 = AtomicU64::new(0);
static ITEM_SET_INSERT_MAX_LEN: AtomicU64 = AtomicU64::new(0);
static ITEM_SET_INSERT_APPENDED: AtomicU64 = AtomicU64::new(0);
static ITEM_SET_INSERT_LAST_EQUAL: AtomicU64 = AtomicU64::new(0);
static ITEM_SET_INSERT_BINARY_SEARCHES: AtomicU64 = AtomicU64::new(0);
static ITEM_SET_INSERT_BINARY_NEW: AtomicU64 = AtomicU64::new(0);
static ITEM_SET_INSERT_BINARY_EXISTING: AtomicU64 = AtomicU64::new(0);
static ITEM_SET_INSERT_SHIFTED_ENTRIES: AtomicU64 = AtomicU64::new(0);
static PARSE_ITEM_CMP_CALLS: AtomicU64 = AtomicU64::new(0);
static PARSE_ITEM_CMP_STEP_ITERATIONS: AtomicU64 = AtomicU64::new(0);
static PARSE_ITEM_EQ_CALLS: AtomicU64 = AtomicU64::new(0);
static PARSE_ITEM_EQ_STEP_ITERATIONS: AtomicU64 = AtomicU64::new(0);
static PARSE_ITEM_HASH_CALLS: AtomicU64 = AtomicU64::new(0);
static PARSE_ITEM_HASH_STEP_ITERATIONS: AtomicU64 = AtomicU64::new(0);

pub fn reset() {
    for counter in counters() {
        counter.store(0, Ordering::Relaxed);
    }
}

pub fn snapshot() -> ProfileSnapshot {
    ProfileSnapshot {
        parse_states_new: load(&PARSE_STATES_NEW),
        parse_states_reused: load(&PARSE_STATES_REUSED),
        transitive_closure_calls: load(&TRANSITIVE_CLOSURE_CALLS),
        transitive_closure_input_entries: load(&TRANSITIVE_CLOSURE_INPUT_ENTRIES),
        transitive_closure_output_entries: load(&TRANSITIVE_CLOSURE_OUTPUT_ENTRIES),
        transitive_closure_max_input_entries: load(&TRANSITIVE_CLOSURE_MAX_INPUT_ENTRIES),
        transitive_closure_max_output_entries: load(&TRANSITIVE_CLOSURE_MAX_OUTPUT_ENTRIES),
        add_item_calls: load(&ADD_ITEM_CALLS),
        add_item_nonterminal_calls: load(&ADD_ITEM_NONTERMINAL_CALLS),
        closure_additions_considered: load(&CLOSURE_ADDITIONS_CONSIDERED),
        inline_sites: load(&INLINE_SITES),
        inline_productions: load(&INLINE_PRODUCTIONS),
        item_set_insert_calls: load(&ITEM_SET_INSERT_CALLS),
        item_set_insert_len_sum: load(&ITEM_SET_INSERT_LEN_SUM),
        item_set_insert_max_len: load(&ITEM_SET_INSERT_MAX_LEN),
        item_set_insert_appended: load(&ITEM_SET_INSERT_APPENDED),
        item_set_insert_last_equal: load(&ITEM_SET_INSERT_LAST_EQUAL),
        item_set_insert_binary_searches: load(&ITEM_SET_INSERT_BINARY_SEARCHES),
        item_set_insert_binary_new: load(&ITEM_SET_INSERT_BINARY_NEW),
        item_set_insert_binary_existing: load(&ITEM_SET_INSERT_BINARY_EXISTING),
        item_set_insert_shifted_entries: load(&ITEM_SET_INSERT_SHIFTED_ENTRIES),
        parse_item_cmp_calls: load(&PARSE_ITEM_CMP_CALLS),
        parse_item_cmp_step_iterations: load(&PARSE_ITEM_CMP_STEP_ITERATIONS),
        parse_item_eq_calls: load(&PARSE_ITEM_EQ_CALLS),
        parse_item_eq_step_iterations: load(&PARSE_ITEM_EQ_STEP_ITERATIONS),
        parse_item_hash_calls: load(&PARSE_ITEM_HASH_CALLS),
        parse_item_hash_step_iterations: load(&PARSE_ITEM_HASH_STEP_ITERATIONS),
    }
}

pub fn parse_state_new() {
    incr(&PARSE_STATES_NEW, 1);
}

pub fn parse_state_reused() {
    incr(&PARSE_STATES_REUSED, 1);
}

pub fn transitive_closure(input_entries: usize, output_entries: usize) {
    incr(&TRANSITIVE_CLOSURE_CALLS, 1);
    incr(&TRANSITIVE_CLOSURE_INPUT_ENTRIES, input_entries as u64);
    incr(&TRANSITIVE_CLOSURE_OUTPUT_ENTRIES, output_entries as u64);
    max(&TRANSITIVE_CLOSURE_MAX_INPUT_ENTRIES, input_entries as u64);
    max(
        &TRANSITIVE_CLOSURE_MAX_OUTPUT_ENTRIES,
        output_entries as u64,
    );
}

pub fn add_item(nonterminal: bool) {
    incr(&ADD_ITEM_CALLS, 1);
    if nonterminal {
        incr(&ADD_ITEM_NONTERMINAL_CALLS, 1);
    }
}

pub fn closure_additions(count: usize) {
    incr(&CLOSURE_ADDITIONS_CONSIDERED, count as u64);
}

pub fn inline_site(production_count: usize) {
    incr(&INLINE_SITES, 1);
    incr(&INLINE_PRODUCTIONS, production_count as u64);
}

pub fn item_set_insert_len(len: usize) {
    incr(&ITEM_SET_INSERT_CALLS, 1);
    incr(&ITEM_SET_INSERT_LEN_SUM, len as u64);
    max(&ITEM_SET_INSERT_MAX_LEN, len as u64);
}

pub fn item_set_insert_appended() {
    incr(&ITEM_SET_INSERT_APPENDED, 1);
}

pub fn item_set_insert_last_equal() {
    incr(&ITEM_SET_INSERT_LAST_EQUAL, 1);
}

pub fn item_set_insert_binary_search() {
    incr(&ITEM_SET_INSERT_BINARY_SEARCHES, 1);
}

pub fn item_set_insert_binary_new(shifted_entries: usize) {
    incr(&ITEM_SET_INSERT_BINARY_NEW, 1);
    incr(&ITEM_SET_INSERT_SHIFTED_ENTRIES, shifted_entries as u64);
}

pub fn item_set_insert_binary_existing() {
    incr(&ITEM_SET_INSERT_BINARY_EXISTING, 1);
}

pub fn parse_item_cmp() {
    incr(&PARSE_ITEM_CMP_CALLS, 1);
}

pub fn parse_item_cmp_step() {
    incr(&PARSE_ITEM_CMP_STEP_ITERATIONS, 1);
}

pub fn parse_item_eq() {
    incr(&PARSE_ITEM_EQ_CALLS, 1);
}

pub fn parse_item_eq_step() {
    incr(&PARSE_ITEM_EQ_STEP_ITERATIONS, 1);
}

pub fn parse_item_hash(steps: usize) {
    incr(&PARSE_ITEM_HASH_CALLS, 1);
    incr(&PARSE_ITEM_HASH_STEP_ITERATIONS, steps as u64);
}

fn counters() -> [&'static AtomicU64; 27] {
    [
        &PARSE_STATES_NEW,
        &PARSE_STATES_REUSED,
        &TRANSITIVE_CLOSURE_CALLS,
        &TRANSITIVE_CLOSURE_INPUT_ENTRIES,
        &TRANSITIVE_CLOSURE_OUTPUT_ENTRIES,
        &TRANSITIVE_CLOSURE_MAX_INPUT_ENTRIES,
        &TRANSITIVE_CLOSURE_MAX_OUTPUT_ENTRIES,
        &ADD_ITEM_CALLS,
        &ADD_ITEM_NONTERMINAL_CALLS,
        &CLOSURE_ADDITIONS_CONSIDERED,
        &INLINE_SITES,
        &INLINE_PRODUCTIONS,
        &ITEM_SET_INSERT_CALLS,
        &ITEM_SET_INSERT_LEN_SUM,
        &ITEM_SET_INSERT_MAX_LEN,
        &ITEM_SET_INSERT_APPENDED,
        &ITEM_SET_INSERT_LAST_EQUAL,
        &ITEM_SET_INSERT_BINARY_SEARCHES,
        &ITEM_SET_INSERT_BINARY_NEW,
        &ITEM_SET_INSERT_BINARY_EXISTING,
        &ITEM_SET_INSERT_SHIFTED_ENTRIES,
        &PARSE_ITEM_CMP_CALLS,
        &PARSE_ITEM_CMP_STEP_ITERATIONS,
        &PARSE_ITEM_EQ_CALLS,
        &PARSE_ITEM_EQ_STEP_ITERATIONS,
        &PARSE_ITEM_HASH_CALLS,
        &PARSE_ITEM_HASH_STEP_ITERATIONS,
    ]
}

fn load(counter: &AtomicU64) -> u64 {
    counter.load(Ordering::Relaxed)
}

fn incr(counter: &AtomicU64, value: u64) {
    counter.fetch_add(value, Ordering::Relaxed);
}

fn max(counter: &AtomicU64, value: u64) {
    let mut current = counter.load(Ordering::Relaxed);
    while value > current {
        match counter.compare_exchange_weak(current, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(next) => current = next,
        }
    }
}
