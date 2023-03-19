use std::cell::RefCell;
use std::collections::{hash_map::Entry, HashMap};
// use guid_create::GUID;

use log::*;
use screeps::{
    find, game, prelude::*, Creep, ObjectId, Part, ResourceType, ReturnCode,
    RoomObjectProperties, Source, StructureController, StructureObject, StructureSpawn, memory,
};

use wasm_bindgen::prelude::*;

mod id;
mod logging;
use id::get_id;

// add wasm_bindgen to any function you would like to expose for call from js
#[wasm_bindgen]
pub fn setup() {
    logging::setup_logging(logging::Debug);
}

// this is one way to persist data between ticks within Rust's memory, as opposed to
// keeping state in memory on game objects - but will be lost on global resets!
thread_local! {
    static CREEP_TARGETS: RefCell<HashMap<String, CreepTarget>> = RefCell::new(HashMap::new());
}

// this enum will represent a creep's lock on a specific target object, storing a js reference to the object id so that we can grab a fresh reference to the object each successive tick, since screeps game objects become 'stale' and shouldn't be used beyond the tick they were fetched
#[derive(Clone, Debug)]
enum CreepTarget {
    Charge(ObjectId<StructureSpawn>),
    Upgrade(ObjectId<StructureController>),
    Harvest(ObjectId<Source>),
}

// to use a reserved name as a function name, use `js_name`:
#[wasm_bindgen(js_name = loop)]
pub fn game_loop() {
    debug!("loop starting! CPU: {}", game::cpu::get_used());
    // mutably borrow the creep_targets refcell, which is holding our creep target locks
    // in the wasm heap
    CREEP_TARGETS.with(|creep_targets_refcell| {
        let mut creep_targets = creep_targets_refcell.borrow_mut();
        debug!("running creeps");
        // same type conversion (and type assumption) as the spawn loop
        for creep in game::creeps().values() {
            run_creep(&creep, &mut creep_targets);
        }
    });

    debug!("running spawns");

    screeps::;

    // Game::spawns returns a `js_sys::Object`, which is a light reference to an
    // object of any kind which is held on the javascript heap.
    //
    // Object::values returns a `js_sys::Array`, which contains the member spawn objects
    // representing all the spawns you control.
    //
    // They are returned as wasm_bindgen::JsValue references, which we can safely
    // assume are StructureSpawn objects as returned from js without checking first
    for spawn in game::spawns().values() {
        // Skip any spawning spawns
        if let Some(_) = spawn.spawning() {
            continue;
        }

        // game::

        if game::creeps().keys().count() >= 8 {
            continue;
        }

        debug!("running spawn {}", String::from(spawn.name()));

        let body = [Part::Move, Part::Move, Part::Carry, Part::Work];

        if spawn.room().unwrap().energy_available() >= body.iter().map(|p| p.cost()).sum() {
            // create a unique name, spawn.
            let name = format!("Role:{}", get_id());

            // note that this bot has a fatal flaw; spawning a creep
            // creates Memory.creeps[creep_name] which will build up forever;
            // these memory entries should be prevented (todo doc link on how) or cleaned up
            let res = spawn.spawn_creep(&body, &name);

            if res != ReturnCode::Ok {
                warn!("couldn't spawn: {:?}", res);
            }
        }
    }

    info!("done! cpu: {}", game::cpu::get_used())
}

fn run_creep(creep: &Creep, creep_targets: &mut HashMap<String, CreepTarget>) {
    if creep.spawning() {
        return;
    }

    let name = creep.try_id().expect("Object has Id").to_string();
    debug!("running creep {}", name);

    let target = creep_targets.entry(name);
    match target {
        Entry::Occupied(entry) => {
            let creep_target = entry.get();
            debug!("Target: {creep_target:?}");

            match creep_target {
                CreepTarget::Upgrade(controller_id)
                    if creep.store().get_used_capacity(Some(ResourceType::Energy)) > 0 =>
                {
                    if let Some(controller) = controller_id.resolve() {
                        let r = creep.upgrade_controller(&controller);
                        if r == ReturnCode::NotInRange {
                            creep.move_to(&controller);
                        } else if r != ReturnCode::Ok {
                            warn!("couldn't upgrade: {:?}", r);
                            entry.remove();
                        }
                    } else {
                        entry.remove();
                    }
                }

                CreepTarget::Harvest(source_id)
                    if creep.store().get_free_capacity(Some(ResourceType::Energy)) > 0 =>
                {
                    if let Some(source) = source_id.resolve() {
                        if creep.pos().is_near_to(source.pos()) {
                            let r = creep.harvest(&source);
                            if r != ReturnCode::Ok {
                                warn!("couldn't harvest: {:?}", r);
                                entry.remove();
                            }
                        } else {
                            creep.move_to(&source);
                        }
                    } else {
                        entry.remove();
                    }
                }

                CreepTarget::Charge(source_id)
                    if creep.store().get_used_capacity(Some(ResourceType::Energy)) > 0 =>
                {
                    if let Some(target) = source_id.resolve() {
                        let r = creep.transfer(&target, ResourceType::Energy, None);
                        if r == ReturnCode::NotInRange {
                            creep.move_to(&target);
                        } else if r != ReturnCode::Ok {
                            warn!("couldn't Transfer: {:?}", r);
                            entry.remove();
                        }
                    } else {
                        entry.remove();
                    }
                }

                _ => {
                    entry.remove();
                }
            };
        }
        Entry::Vacant(entry) => {
            // no target, let's find one depending on if we have energy
            let room = creep.room().expect("couldn't resolve creep room");

            if creep.store().get_used_capacity(Some(ResourceType::Energy)) > 0 {
                // room.find(find::STRUCTURES, Some());

                let structures = room.find(find::STRUCTURES, None);

                let mut spawners: Vec<&screeps::StructureSpawn> = Vec::new();
                let mut controller: Option<&screeps::StructureController> = None;

                for structure in structures.iter() {
                    if let StructureObject::StructureSpawn(spawn) = structure {
                        if spawn.store().get_free_capacity(Some(ResourceType::Energy)) > 0 {
                            spawners.push(spawn);
                        }
                        continue;
                    }
                    if let StructureObject::StructureController(ctrl) = structure {
                        controller = Some(ctrl);
                        continue;
                    }
                }

                if spawners.len() > 0 {
                    entry.insert(CreepTarget::Charge(spawners[0].id()));
                    return;
                }

                if let Some(controller) = controller {
                    entry.insert(CreepTarget::Upgrade(controller.id()));
                    return;
                }

                error!("No Controller could be found");
            } else if let Some(source) = room.find(find::SOURCES_ACTIVE, None).get(0) {
                entry.insert(CreepTarget::Harvest(source.id()));
            }
        }
    }
}

/*
 */
