//! Generated CREATURE entity spawn descriptor table integration tests (#685).
//!
//! Lives outside `src/generated/` so `tools/rivet-codegen`'s golden drift test
//! keeps owning the exact file set in `src/generated/`.

use crate::generated::entity_spawn_descriptors::{
    CREATURE_ENTITY_SPAWN_DESCRIPTORS, EntitySpawnHeightmap, EntitySpawnPlacement,
    creature_spawn_descriptor_by_id, creature_spawn_descriptor_by_name,
};

#[test]
fn creature_descriptor_table_has_exact_19_ordered_entries() {
    assert_eq!(CREATURE_ENTITY_SPAWN_DESCRIPTORS.len(), 19);

    for pair in CREATURE_ENTITY_SPAWN_DESCRIPTORS.windows(2) {
        assert!(
            pair[0].id < pair[1].id,
            "descriptors must be sorted by strictly ascending registry id: {} >= {}",
            pair[0].id,
            pair[1].id,
        );
    }

    for desc in CREATURE_ENTITY_SPAWN_DESCRIPTORS {
        assert_eq!(desc.category, "creature");
        assert!(desc.can_summon);
        assert!(desc.allowed_in_peaceful);
        assert!(desc.width > 0.0 && desc.width.is_finite());
        assert!(desc.height > 0.0 && desc.height.is_finite());
        assert!(desc.spawn_dimensions_scale > 0.0 && desc.spawn_dimensions_scale.is_finite());
        assert!(desc.factory.contains("::"));
        assert!(desc.spawn_predicate.contains("::"));
        assert_eq!(creature_spawn_descriptor_by_id(desc.id), Some(desc));
        assert_eq!(creature_spawn_descriptor_by_name(desc.name), Some(desc));
    }

    assert_eq!(creature_spawn_descriptor_by_id(0), None);
    assert_eq!(creature_spawn_descriptor_by_id(u16::MAX), None);
    assert_eq!(creature_spawn_descriptor_by_name("minecraft:zombie"), None);
}

#[test]
fn creature_descriptor_anchors_match_paper_26_2() {
    let armadillo = creature_spawn_descriptor_by_name("minecraft:armadillo").unwrap();
    assert_eq!(armadillo.id, 4);
    assert_eq!(armadillo.placement, EntitySpawnPlacement::OnGround);
    assert_eq!(
        armadillo.heightmap,
        EntitySpawnHeightmap::MotionBlockingNoLeaves
    );
    assert_eq!(
        armadillo.spawn_predicate,
        "net.minecraft.world.entity.animal.armadillo.Armadillo::checkArmadilloSpawnRules"
    );

    let fox = creature_spawn_descriptor_by_name("minecraft:fox").unwrap();
    assert_eq!(fox.id, 54);
    assert_eq!(fox.placement, EntitySpawnPlacement::NoRestrictions);
    assert_eq!(fox.heightmap, EntitySpawnHeightmap::MotionBlockingNoLeaves);
    assert_eq!(fox.immune_to_tag, "minecraft:fox_immune_to");
    assert_eq!(
        fox.spawn_predicate,
        "net.minecraft.world.entity.animal.fox.Fox::checkFoxSpawnRules"
    );

    let panda = creature_spawn_descriptor_by_name("minecraft:panda").unwrap();
    assert_eq!(panda.id, 96);
    assert_eq!(panda.placement, EntitySpawnPlacement::NoRestrictions);
    assert_eq!(
        panda.heightmap,
        EntitySpawnHeightmap::MotionBlockingNoLeaves
    );
    assert_eq!(
        panda.spawn_predicate,
        "net.minecraft.world.entity.animal.Animal::checkAnimalSpawnRules"
    );

    let parrot = creature_spawn_descriptor_by_name("minecraft:parrot").unwrap();
    assert_eq!(parrot.id, 98);
    assert_eq!(parrot.placement, EntitySpawnPlacement::OnGround);
    assert_eq!(parrot.heightmap, EntitySpawnHeightmap::MotionBlocking);
    assert_eq!(
        parrot.spawn_predicate,
        "net.minecraft.world.entity.animal.parrot.Parrot::checkParrotSpawnRules"
    );

    let polar_bear = creature_spawn_descriptor_by_name("minecraft:polar_bear").unwrap();
    assert_eq!(polar_bear.id, 104);
    assert_eq!(polar_bear.placement, EntitySpawnPlacement::OnGround);
    assert_eq!(polar_bear.immune_to_tag, "minecraft:polar_bear_immune_to");
    assert_eq!(
        polar_bear.spawn_predicate,
        "net.minecraft.world.entity.animal.polarbear.PolarBear::checkPolarBearSpawnRules"
    );

    let wolf = creature_spawn_descriptor_by_name("minecraft:wolf").unwrap();
    assert_eq!(wolf.id, 149);
    assert_eq!(wolf.placement, EntitySpawnPlacement::OnGround);
    assert_eq!(wolf.heightmap, EntitySpawnHeightmap::MotionBlockingNoLeaves);
    assert_eq!(
        wolf.spawn_predicate,
        "net.minecraft.world.entity.animal.wolf.Wolf::checkWolfSpawnRules"
    );
}
