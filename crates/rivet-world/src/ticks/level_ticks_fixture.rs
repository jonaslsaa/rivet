//! Differential execution of genuine Paper LevelTicks command transcripts.
use super::*;
use crate::ticks::{SavedTick, TickPriority};
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[derive(Default)]
struct Store(Vec<(ChunkPos, LevelChunkTicks<String>)>);
impl TickContainers<String> for Store {
    fn get(&self, pos: ChunkPos) -> Option<&LevelChunkTicks<String>> {
        self.0.iter().find(|(p, _)| *p == pos).map(|(_, c)| c)
    }
    fn get_mut(&mut self, pos: ChunkPos) -> Option<&mut LevelChunkTicks<String>> {
        self.0.iter_mut().find(|(p, _)| *p == pos).map(|(_, c)| c)
    }
}
#[derive(Default)]
struct State {
    ticks: LevelTicks<String>,
    store: Store,
}
fn number(c: &Value, index: usize) -> i64 {
    c[index].as_i64().unwrap()
}
fn int(c: &Value, index: usize) -> i32 {
    number(c, index) as i32
}
fn pos(c: &Value, index: usize) -> ChunkPos {
    ChunkPos::new(int(c, index), int(c, index + 1))
}
fn scheduled(c: &Value, start: usize) -> ScheduledTick<String> {
    ScheduledTick::new(
        c[start].as_str().unwrap().to_owned(),
        BlockPos::new(int(c, start + 1), int(c, start + 2), int(c, start + 3)),
        number(c, start + 4),
        TickPriority::by_value(int(c, start + 5)),
        number(c, start + 6),
    )
}
fn bounds(c: &Value) -> BoundingBox {
    BoundingBox::new(std::array::from_fn(|i| int(c, i)))
}
fn offset(c: &Value) -> Vec3i {
    Vec3i::new(int(c, 0), int(c, 1), int(c, 2))
}
fn tick_json(t: &ScheduledTick<String>) -> Value {
    json!([
        t.r#type,
        t.pos.get_x(),
        t.pos.get_y(),
        t.pos.get_z(),
        t.trigger_tick,
        t.priority.value(),
        t.sub_tick_order
    ])
}
fn snapshot(s: &State) -> Value {
    json!({
        "containers": s.store.0.iter().map(|(pos,c)|json!({"pos":[pos.x(),pos.z()],"count":c.count(),"ticks":c.all().map(tick_json).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "index":s.ticks.next.lock().unwrap().entries().map(|(key,value)|json!([key,value])).collect::<Vec<_>>(),
        "to_run":s.ticks.to_run.iter().map(tick_json).collect::<Vec<_>>(),
        "already_run":s.ticks.already_run.iter().map(tick_json).collect::<Vec<_>>(),
        "count":s.ticks.count(&s.store)
    })
}

#[test]
fn actual_paper_scheduler_commands_match_order_queries_and_partial_state() {
    let fixture = include_str!("../../../../tools/rivet-oracle/fixtures/level-ticks.jsonl");
    let mut cases = 0;
    for line in fixture.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let mut states = [State::default(), State::default()];
        let mut events = Vec::new();
        let mut snapshots = row["snapshots"].as_array().unwrap().iter().peekable();
        for (step, c) in row["commands"].as_array().unwrap().iter().enumerate() {
            let target = int(c, 0) as usize;
            let op = c[1].as_str().unwrap();
            let s = &mut states[target];
            let mut interrupted = false;
            match op {
                "add" => {
                    let p = pos(c, 2);
                    let container = if let Some(pending) = c.get(4) {
                        LevelChunkTicks::new_with_pending(
                            pending
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|c| {
                                    let t = scheduled(c, 0);
                                    SavedTick::new(
                                        t.r#type,
                                        t.pos,
                                        t.trigger_tick as i32,
                                        t.priority,
                                    )
                                })
                                .collect(),
                        )
                    } else {
                        LevelChunkTicks::new()
                    };
                    if let Some(previous) = s.store.get_mut(p) {
                        *previous = container;
                    } else {
                        s.store.0.push((p, container));
                    }
                    s.ticks.add_container(p, s.store.get_mut(p).unwrap());
                }
                "remove" => {
                    let p = pos(c, 2);
                    s.ticks.remove_container(p, s.store.get_mut(p).unwrap());
                }
                "unpack" => s.store.get_mut(pos(c, 2)).unwrap().unpack(number(c, 4)),
                "schedule" => {
                    s.ticks.schedule(&mut s.store, scheduled(c, 2));
                }
                "direct" => {
                    let tick = scheduled(c, 2);
                    s.store
                        .get_mut(ChunkPos::containing(&tick.pos))
                        .unwrap()
                        .schedule(tick);
                }
                "clear" => s.ticks.clear_area(&mut s.store, bounds(&c[2])),
                "copy" => s
                    .ticks
                    .copy_area(&mut s.store, bounds(&c[2]), offset(&c[3])),
                "copy_from" => {
                    let (a, b) = states.split_at_mut(1);
                    let (destination, source) = if target == 0 {
                        (&mut a[0], &b[0])
                    } else {
                        (&mut b[0], &a[0])
                    };
                    destination.ticks.copy_area_from(
                        &mut destination.store,
                        &source.ticks,
                        &source.store,
                        bounds(&c[2]),
                        offset(&c[3]),
                    );
                }
                "tick" => {
                    let now = number(c, 2);
                    let blocked: HashSet<_> = c[4]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|c| pos(c, 0).pack())
                        .collect();
                    let mode = c[5].as_str().unwrap();
                    let mut first = true;
                    interrupted=catch_unwind(AssertUnwindSafe(||s.ticks.tick(&mut s.store,now,int(c,3),|p|!blocked.contains(&p.pack()),|ticks,store,p,kind|{
                        let queries:Vec<_>=(0..7).map(|x|ticks.will_tick_this_tick(&BlockPos::new(x,64,0),&"t".to_owned())).collect();
                        events.push(json!({"tick":[kind,p.get_x(),p.get_y(),p.get_z()],"queries":queries,"count":ticks.count(store)}));
                        if first {
                            first=false;
                            match mode {
                                "reschedule"=>{ticks.schedule(store,ScheduledTick::new(kind,p,now,TickPriority::Normal,99));}
                                "clear"=>ticks.clear_area(store,BoundingBox::new([1,0,0,1,100,0])),
                                "copy"=>ticks.copy_area(store,BoundingBox::new([-32,-64,-32,63,319,31]),Vec3i::new(0,0,16)),
                                "throw"=>panic!("source sentinel"),
                                _=>{}
                            }
                        }
                    }))).is_err();
                }
                _ => panic!("unknown oracle command {op}"),
            }
            if snapshots
                .peek()
                .is_some_and(|snapshot| snapshot["step"].as_u64() == Some(step as u64))
            {
                let expected = snapshots.next().unwrap();
                let actual = json!({"step":step,"a":snapshot(&states[0]),"b":snapshot(&states[1]),"interrupted":interrupted});
                assert_eq!(&actual, expected, "{} step{step} {op}", row["name"]);
            }
        }
        assert!(snapshots.next().is_none());
        assert_eq!(
            json!(events),
            row["events"],
            "{} callback order/queries",
            row["name"]
        );
        cases += 1;
    }
    assert_eq!(cases, 13);
}
