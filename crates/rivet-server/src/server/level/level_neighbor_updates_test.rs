//! Genuine Java private CollectingNeighborUpdater callback traces. The driver
//! controls only callback bodies; it executes the production Rust queue loop.
use super::*;
use serde_json::{Value, json};

struct Driver {
    updater: LiveNeighborUpdater,
    trace: Vec<String>,
    failure: String,
}
impl Driver {
    fn task(&mut self, id: i32) {
        self.updater.enqueue(
            BlockPos::new(id, 64, 0),
            Update::Simple {
                pos: BlockPos::new(id, 64, 0),
                source: Block::from_name("minecraft:stone").unwrap(),
            },
        );
    }
}
impl NeighborDriver for Driver {
    fn updater(&mut self) -> &mut LiveNeighborUpdater {
        &mut self.updater
    }
    fn debug(&mut self, positions: &[BlockPos]) -> Result<()> {
        for pos in positions {
            self.trace.push(format!("debug:{}", pos.get_x()));
            if self.failure == "debug" && pos.get_x() == 2 {
                return Err(unavailable("debug"));
            }
        }
        Ok(())
    }
    fn execute(&mut self, update: Update) -> Result<()> {
        let Update::Simple { pos, .. } = update else {
            panic!("controlled Java NeighborUpdates proxy is a single-step update");
        };
        let id = pos.get_x();
        self.trace.push(format!("run:{id}"));
        if id == 1 {
            self.task(2);
            self.task(3);
        }
        if id == 2 {
            if self.failure == "callback" {
                return Err(unavailable("callback"));
            }
            if self.failure == "panic" {
                std::panic::panic_any("source callback panic");
            }
            self.task(4);
        }
        Ok(())
    }
}
#[test]
fn actual_paper_collecting_queue_reentrancy_limits_and_finally_cleanup() {
    let mut cases = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/neighbor-collector.jsonl").lines()
    {
        let row: Value = serde_json::from_str(line).unwrap();
        let mut driver = Driver {
            updater: LiveNeighborUpdater::new(row["max"].as_i64().unwrap() as i32),
            trace: Vec::new(),
            failure: row["failure"].as_str().unwrap().into(),
        };
        driver.updater.set_debug_listener_enabled(true);
        driver.task(1);
        let result = drain_and_clear(&mut driver);
        assert_eq!(result.is_err(), row["failed"].as_bool().unwrap(), "{row}");
        assert_eq!(json!(driver.trace), row["trace"], "{row}");
        assert_eq!(json!(driver.updater.count), row["count"]);
        assert_eq!(json!(driver.updater.stack.len()), row["stack"]);
        assert_eq!(json!(driver.updater.added.len()), row["addedThisLayer"]);
        cases += 1;
    }
    assert_eq!(cases, 21);
}
#[test]
fn original_callback_panic_is_rethrown_after_scheduling_state_clears() {
    let mut driver = Driver {
        updater: LiveNeighborUpdater::new(16),
        trace: Vec::new(),
        failure: "panic".into(),
    };
    driver.task(1);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drain_and_clear(&mut driver)
    }))
    .unwrap_err();
    assert_eq!(panic.downcast_ref::<&str>(), Some(&"source callback panic"));
    assert_eq!(driver.trace, ["run:1", "run:2"]);
    assert_eq!(driver.updater.count, 0);
    assert!(driver.updater.stack.is_empty());
    assert!(driver.updater.added.is_empty());
}
