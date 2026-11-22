use super::*;
use bytes::BytesMut;
use rivet_core::ChatFormatting;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::tag::Tag;
use rivet_protocol::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_registry::RegistryAccess;
use rivet_text::{Component, Style};

fn make_buf() -> RegistryFriendlyByteBuf {
    RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty())
}

fn roundtrip_packet(packet: &ScoreboardPacket) -> ScoreboardPacket {
    let mut out = make_buf();
    packet
        .encode_framed(&mut out)
        .expect("packet encode succeeds");
    let bytes = out.into_inner();
    let mut input = RegistryFriendlyByteBuf::new(bytes, RegistryAccess::empty());
    let decoded = ScoreboardPacket::decode_framed(&mut input).expect("packet decode succeeds");
    assert_eq!(
        input.inner().readable_bytes(),
        0,
        "all bytes consumed for packet id {}",
        packet.protocol_id()
    );
    decoded
}

#[test]
fn criteria_builtin_and_dynamic_lookup() {
    let dummy = ObjectiveCriteria::by_name("dummy").expect("dummy exists");
    assert!(!dummy.is_read_only());
    assert_eq!(dummy.default_render_type(), RenderType::Integer);

    let trigger = ObjectiveCriteria::by_name("trigger").expect("trigger exists");
    assert!(!trigger.is_read_only());
    assert_eq!(trigger.default_render_type(), RenderType::Integer);

    let health = ObjectiveCriteria::by_name("health").expect("health exists");
    assert!(health.is_read_only());
    assert_eq!(health.default_render_type(), RenderType::Hearts);

    let death_count = ObjectiveCriteria::by_name("deathCount").expect("deathCount exists");
    assert!(!death_count.is_read_only());
    assert_eq!(death_count.default_render_type(), RenderType::Integer);

    let food = ObjectiveCriteria::by_name("food").expect("food exists");
    assert!(food.is_read_only());

    let team_kill = ObjectiveCriteria::by_name("teamkill.red").expect("teamkill.red exists");
    assert!(!team_kill.is_read_only());

    let killed_by = ObjectiveCriteria::by_name("killedByTeam.dark_blue")
        .expect("killedByTeam.dark_blue exists");
    assert!(!killed_by.is_read_only());

    let custom_stat =
        ObjectiveCriteria::by_name("minecraft.custom:minecraft.jump").expect("stat criterion");
    assert!(!custom_stat.is_read_only());
    assert_eq!(custom_stat.default_render_type(), RenderType::Integer);

    assert!(ObjectiveCriteria::by_name("nonexistent_criterion").is_none());
}

#[test]
fn display_slot_ids_and_names_match_paper() {
    assert_eq!(DisplaySlot::ALL.len(), 19);
    for (idx, &slot) in DisplaySlot::ALL.iter().enumerate() {
        assert_eq!(slot.id(), idx as i32);
        assert_eq!(DisplaySlot::from_id(idx as i32), Some(slot));
        assert_eq!(DisplaySlot::from_name(slot.serialized_name()), Some(slot));
    }
    assert_eq!(DisplaySlot::List.serialized_name(), "list");
    assert_eq!(DisplaySlot::Sidebar.serialized_name(), "sidebar");
    assert_eq!(DisplaySlot::BelowName.serialized_name(), "below_name");
    assert_eq!(DisplaySlot::TeamRed.serialized_name(), "sidebar.team.red");
    assert_eq!(
        DisplaySlot::team_color_to_slot(TeamColor::Gold),
        DisplaySlot::TeamGold
    );
    assert_eq!(DisplaySlot::from_id(-1), None);
    assert_eq!(DisplaySlot::from_id(19), None);
    assert_eq!(DisplaySlot::from_name("unknown"), None);
}

#[test]
fn objective_lifecycle_and_criteria_indexing() {
    let mut sb = ServerScoreboard::new();
    let dummy = ObjectiveCriteria::dummy();
    let health = ObjectiveCriteria::health();

    sb.add_objective(
        "kills",
        dummy.clone(),
        Component::literal("Kills"),
        RenderType::Integer,
        false,
        None,
    )
    .expect("add kills");

    // Duplicate objective name rejected
    let dup = sb.add_objective(
        "kills",
        dummy.clone(),
        Component::literal("Dup"),
        RenderType::Integer,
        false,
        None,
    );
    assert!(matches!(dup, Err(ScoreboardError::DuplicateObjective(_))));

    sb.add_objective(
        "hp",
        health.clone(),
        Component::literal("HP"),
        RenderType::Hearts,
        true,
        Some(NumberFormat::sidebar_default()),
    )
    .expect("add hp");

    assert_eq!(sb.scoreboard().objectives().count(), 2);
    assert_eq!(sb.scoreboard().objectives_by_criteria(&dummy).len(), 1);
    assert_eq!(sb.scoreboard().objectives_by_criteria(&health).len(), 1);

    // Mutate untracked objective: marks dirty, no packet emitted
    sb.clear_dirty();
    sb.drain_packets();
    sb.set_objective_display_name("kills", Component::literal("Total Kills"))
        .expect("set display name");
    sb.set_objective_render_type("kills", RenderType::Hearts)
        .expect("set render type");
    sb.set_objective_display_auto_update("kills", true)
        .expect("set auto update");
    sb.set_objective_number_format("kills", Some(NumberFormat::Blank))
        .expect("set number format");
    assert!(sb.is_dirty());
    assert!(sb.drain_packets().is_empty());

    let obj = sb.get_objective("kills").expect("kills exists");
    assert_eq!(obj.display_name(), &Component::literal("Total Kills"));
    assert_eq!(obj.render_type(), RenderType::Hearts);
    assert!(obj.display_auto_update());
    assert_eq!(obj.number_format(), Some(&NumberFormat::Blank));
}

#[test]
fn score_mutation_readonly_locking_wrapping_and_auto_update() {
    let mut sb = ServerScoreboard::new();
    sb.add_objective(
        "pts",
        ObjectiveCriteria::dummy(),
        Component::literal("Points"),
        RenderType::Integer,
        true,
        None,
    )
    .unwrap();
    sb.add_objective(
        "hp",
        ObjectiveCriteria::health(),
        Component::literal("Health"),
        RenderType::Hearts,
        false,
        None,
    )
    .unwrap();

    let alice = ScoreHolder::with_display_name("Alice", Component::literal("Lady Alice"));

    // Read-only objective rejects normal access
    let err = sb.mutate_player_score(&alice, "hp", false, |access| access.set(20));
    assert!(matches!(err, Err(ScoreboardError::ReadOnlyScore)));

    // force_allow_modify succeeds on read-only objective
    sb.mutate_player_score(&alice, "hp", true, |access| access.set(20))
        .expect("forced write to read-only objective");
    assert_eq!(
        sb.get_player_score_info(&alice, "hp").map(|s| s.value),
        Some(20)
    );

    // for_all_objectives uses force_allow_modify=true
    sb.for_all_objectives(&ObjectiveCriteria::health(), &alice, |access| {
        access.add(-5)?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        sb.get_player_score_info(&alice, "hp").map(|s| s.value),
        Some(15)
    );

    // Newly created score starts locked, display_auto_update populates display name,
    // and Java wrapping i32 arithmetic applies on overflow.
    sb.mutate_player_score(&alice, "pts", false, |access| {
        assert!(access.is_locked());
        access.unlock();
        assert!(!access.is_locked());
        access.set(i32::MAX)?;
        access.increment()?;
        assert_eq!(access.get(), i32::MIN);
        access.set_number_format_override(Some(NumberFormat::Fixed(Component::literal("MAX"))));
        Ok(())
    })
    .unwrap();

    let info = sb.get_player_score_info(&alice, "pts").unwrap();
    assert_eq!(info.value, i32::MIN);
    assert!(!info.locked);
    assert_eq!(
        info.number_format,
        Some(NumberFormat::Fixed(Component::literal("MAX")))
    );
    let entries = sb.list_player_scores("pts");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].display, Some(Component::literal("Lady Alice")));
}

#[test]
fn tracked_objective_packet_sequences_and_display_slot_transitions() {
    let mut sb = ServerScoreboard::new();
    sb.add_objective(
        "obj1",
        ObjectiveCriteria::dummy(),
        Component::literal("Obj 1"),
        RenderType::Integer,
        false,
        None,
    )
    .unwrap();
    sb.add_objective(
        "obj2",
        ObjectiveCriteria::dummy(),
        Component::literal("Obj 2"),
        RenderType::Integer,
        false,
        None,
    )
    .unwrap();

    let alice = ScoreHolder::for_name_only("Alice");
    let bob = ScoreHolder::for_name_only("Bob");
    sb.mutate_player_score(&alice, "obj1", false, |a| a.set(10))
        .unwrap();
    sb.mutate_player_score(&bob, "obj1", false, |a| a.set(20))
        .unwrap();

    // Before tracking: no packets emitted for untracked scores
    assert!(sb.drain_packets().is_empty());

    // Assign obj1 to Sidebar -> starts tracking:
    // Paper order: SetObjective(ADD), SetDisplayObjective(Sidebar, Some("obj1")), SetScore(Alice), SetScore(Bob)
    // plus SetDisplayObjective for the slot assignment.
    sb.set_display_objective(DisplaySlot::Sidebar, Some("obj1"))
        .unwrap();
    let packets = sb.drain_packets();
    assert!(matches!(
        &packets[0],
        ScoreboardPacket::SetObjective(p)
            if p.objective_name == "obj1" && p.method == ClientboundSetObjectivePacket::METHOD_ADD
    ));
    assert!(matches!(
        &packets[1],
        ScoreboardPacket::SetDisplayObjective(p)
            if p.slot == DisplaySlot::Sidebar && p.objective_name() == Some("obj1")
    ));
    assert!(matches!(
        &packets[2],
        ScoreboardPacket::SetScore(p) if p.owner == "Alice" && p.objective_name == "obj1" && p.score == 10
    ));
    assert!(matches!(
        &packets[3],
        ScoreboardPacket::SetScore(p) if p.owner == "Bob" && p.objective_name == "obj1" && p.score == 20
    ));

    // Also assign obj1 to List slot -> already tracked, only SetDisplayObjective emitted
    sb.set_display_objective(DisplaySlot::List, Some("obj1"))
        .unwrap();
    let packets = sb.drain_packets();
    assert_eq!(packets.len(), 1);
    assert!(matches!(
        &packets[0],
        ScoreboardPacket::SetDisplayObjective(p)
            if p.slot == DisplaySlot::List && p.objective_name() == Some("obj1")
    ));

    // Mutating a tracked objective emits SetObjective(CHANGE)
    sb.set_objective_display_name("obj1", Component::literal("Renamed"))
        .unwrap();
    let packets = sb.drain_packets();
    assert_eq!(packets.len(), 1);
    assert!(matches!(
        &packets[0],
        ScoreboardPacket::SetObjective(p)
            if p.objective_name == "obj1" && p.method == ClientboundSetObjectivePacket::METHOD_CHANGE
    ));

    // Mutating a score on a tracked objective emits SetScore
    sb.mutate_player_score(&alice, "obj1", false, |a| a.set(15))
        .unwrap();
    let packets = sb.drain_packets();
    assert_eq!(packets.len(), 1);
    assert!(matches!(
        &packets[0],
        ScoreboardPacket::SetScore(p) if p.owner == "Alice" && p.score == 15
    ));

    // Clearing Sidebar while List still displays obj1 does NOT stop tracking obj1
    sb.set_display_objective(DisplaySlot::Sidebar, None)
        .unwrap();
    assert!(sb.is_objective_tracked("obj1"));
    let packets = sb.drain_packets();
    assert_eq!(packets.len(), 1);
    assert!(matches!(
        &packets[0],
        ScoreboardPacket::SetDisplayObjective(p)
            if p.slot == DisplaySlot::Sidebar && p.objective_name().is_none()
    ));

    // Clearing List (last slot displaying obj1) DOES stop tracking obj1,
    // emitting SetObjective(REMOVE) (the slot was already cleared before stopTrackingObjective).
    sb.set_display_objective(DisplaySlot::List, None).unwrap();
    assert!(!sb.is_objective_tracked("obj1"));
    let packets = sb.drain_packets();
    assert_eq!(packets.len(), 1);
    assert!(matches!(
        &packets[0],
        ScoreboardPacket::SetObjective(o)
            if o.objective_name == "obj1" && o.method == ClientboundSetObjectivePacket::METHOD_REMOVE
    ));

    // Resetting a single score on an untracked objective emits NO packet
    sb.reset_single_player_score(&alice, "obj1");
    assert!(sb.drain_packets().is_empty());

    // Re-track obj1, reset Bob's single score -> emits ResetScore(Bob, Some("obj1"))
    sb.set_display_objective(DisplaySlot::Sidebar, Some("obj1"))
        .unwrap();
    sb.drain_packets();
    sb.reset_single_player_score(&bob, "obj1");
    let packets = sb.drain_packets();
    assert_eq!(packets.len(), 1);
    assert!(matches!(
        &packets[0],
        ScoreboardPacket::ResetScore(p)
            if p.owner == "Bob" && p.objective_name.as_deref() == Some("obj1")
    ));

    // Resetting all scores for a player emits ResetScore(owner, None)
    sb.mutate_player_score(&alice, "obj2", false, |a| a.set(5))
        .unwrap();
    sb.drain_packets();
    sb.reset_all_player_scores(&alice);
    let packets = sb.drain_packets();
    assert_eq!(packets.len(), 1);
    assert!(matches!(
        &packets[0],
        ScoreboardPacket::ResetScore(p) if p.owner == "Alice" && p.objective_name.is_none()
    ));

    // Deleting a tracked objective clears its display slot and emits REMOVE
    sb.remove_objective("obj1");
    assert!(!sb.is_objective_tracked("obj1"));
    assert!(sb.get_display_objective(DisplaySlot::Sidebar).is_none());
    let packets = sb.drain_packets();
    assert!(packets.iter().any(|p| matches!(
        p,
        ScoreboardPacket::SetObjective(o)
            if o.objective_name == "obj1" && o.method == ClientboundSetObjectivePacket::METHOD_REMOVE
    )));
}

#[test]
fn teams_lifecycle_membership_transfer_and_properties() {
    let mut sb = ServerScoreboard::new();

    sb.add_player_team("red").expect("add red");
    assert!(matches!(
        sb.add_player_team("red"),
        Err(ScoreboardError::DuplicateTeam(_))
    ));
    sb.add_player_team("blue").expect("add blue");

    let packets = sb.drain_packets();
    assert_eq!(packets.len(), 2);
    assert!(matches!(
        &packets[0],
        ScoreboardPacket::SetPlayerTeam(p)
            if p.name == "red" && p.method == ClientboundSetPlayerTeamPacket::METHOD_ADD
    ));

    // Configure team properties
    sb.set_team_display_name("red", Component::literal("Red Team"))
        .unwrap();
    sb.set_team_prefix("red", Some(Component::literal("[RED] ")))
        .unwrap();
    sb.set_team_suffix("red", Some(Component::literal(" *")))
        .unwrap();
    sb.set_team_color("red", Some(TeamColor::Red)).unwrap();
    sb.set_team_allow_friendly_fire("red", false).unwrap();
    sb.set_team_see_friendly_invisibles("red", false).unwrap();
    sb.set_team_name_tag_visibility("red", Visibility::HideForOtherTeams)
        .unwrap();
    sb.set_team_death_message_visibility("red", Visibility::Never)
        .unwrap();
    sb.set_team_collision_rule("red", CollisionRule::PushOwnTeam)
        .unwrap();

    let red = sb.get_player_team("red").unwrap();
    assert_eq!(red.pack_options(), 0);
    assert_eq!(red.color(), Some(TeamColor::Red));
    assert_eq!(red.name_tag_visibility(), Visibility::HideForOtherTeams);
    assert_eq!(red.death_message_visibility(), Visibility::Never);
    assert_eq!(red.collision_rule(), CollisionRule::PushOwnTeam);
    let _display = red.formatted_display_name();
    let formatted = red.formatted_name(&Component::literal("Steve"));
    assert_eq!(formatted.get_siblings().len(), 3);

    sb.drain_packets();

    // Add ordered members to red
    assert!(sb.add_player_to_team("Alice", "red").unwrap());
    assert!(sb.add_player_to_team("Bob", "red").unwrap());
    assert!(sb.add_player_to_team("Charlie", "red").unwrap());
    let members: Vec<&str> = sb
        .get_player_team("red")
        .unwrap()
        .players()
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(members, vec!["Alice", "Bob", "Charlie"]);

    // Transfer Bob from red -> blue: removes from red first, then joins blue
    sb.drain_packets();
    assert!(sb.add_player_to_team("Bob", "blue").unwrap());
    assert_eq!(
        sb.get_players_team("Bob").map(PlayerTeam::name),
        Some("blue")
    );
    let red_members: Vec<&str> = sb
        .get_player_team("red")
        .unwrap()
        .players()
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(red_members, vec!["Alice", "Charlie"]);

    let packets = sb.drain_packets();
    assert_eq!(packets.len(), 2);
    assert!(matches!(
        &packets[0],
        ScoreboardPacket::SetPlayerTeam(p)
            if p.name == "red" && p.method == ClientboundSetPlayerTeamPacket::METHOD_LEAVE && p.players == vec!["Bob"]
    ));
    assert!(matches!(
        &packets[1],
        ScoreboardPacket::SetPlayerTeam(p)
            if p.name == "blue" && p.method == ClientboundSetPlayerTeamPacket::METHOD_JOIN && p.players == vec!["Bob"]
    ));

    // Removing a player from the wrong team fails
    let wrong = sb.remove_player_from_team("Bob", "red");
    assert!(matches!(wrong, Err(ScoreboardError::PlayerNotOnTeam(_))));

    // Removing a team cleans up its members
    sb.drain_packets();
    assert!(sb.remove_player_team("blue").is_some());
    assert!(sb.get_players_team("Bob").is_none());
    let packets = sb.drain_packets();
    assert_eq!(packets.len(), 1);
    assert!(matches!(
        &packets[0],
        ScoreboardPacket::SetPlayerTeam(p)
            if p.name == "blue" && p.method == ClientboundSetPlayerTeamPacket::METHOD_REMOVE
    ));
}

#[test]
fn initial_sync_packets_send_teams_then_unique_displayed_objectives() {
    let mut sb = ServerScoreboard::new();
    sb.add_player_team("t1").unwrap();
    sb.add_player_to_team("Alice", "t1").unwrap();
    sb.add_objective(
        "score",
        ObjectiveCriteria::dummy(),
        Component::literal("Score"),
        RenderType::Integer,
        false,
        None,
    )
    .unwrap();
    sb.mutate_player_score(&ScoreHolder::for_name_only("Alice"), "score", false, |a| {
        a.set(42)
    })
    .unwrap();
    // Display the same objective in two slots; initial sync must track it only once.
    sb.set_display_objective(DisplaySlot::Sidebar, Some("score"))
        .unwrap();
    sb.set_display_objective(DisplaySlot::List, Some("score"))
        .unwrap();

    let sync = sb.initial_sync_packets_for_player();
    // Expected order:
    // 0: SetPlayerTeam(ADD, "t1")
    // 1: SetObjective(ADD, "score")
    // 2: SetDisplayObjective(List, Some("score"))
    // 3: SetDisplayObjective(Sidebar, Some("score"))
    // 4: SetScore("Alice", "score", 42)
    assert_eq!(sync.len(), 5);
    assert!(matches!(
        &sync[0],
        ScoreboardPacket::SetPlayerTeam(p)
            if p.name == "t1" && p.method == ClientboundSetPlayerTeamPacket::METHOD_ADD
    ));
    assert!(matches!(
        &sync[1],
        ScoreboardPacket::SetObjective(p)
            if p.objective_name == "score" && p.method == ClientboundSetObjectivePacket::METHOD_ADD
    ));
    assert!(matches!(
        &sync[2],
        ScoreboardPacket::SetDisplayObjective(p) if p.slot == DisplaySlot::List
    ));
    assert!(matches!(
        &sync[3],
        ScoreboardPacket::SetDisplayObjective(p) if p.slot == DisplaySlot::Sidebar
    ));
    assert!(matches!(
        &sync[4],
        ScoreboardPacket::SetScore(p) if p.owner == "Alice" && p.score == 42
    ));
}

#[test]
fn number_format_and_all_packets_wire_roundtrip() {
    let formats = vec![
        NumberFormat::Blank,
        NumberFormat::no_style(),
        NumberFormat::sidebar_default(),
        NumberFormat::player_list_default(),
        NumberFormat::Fixed(Component::literal("FIXED")),
    ];
    for fmt in &formats {
        let mut out = make_buf();
        fmt.encode_stream(&mut out).expect("encode format");
        let mut input = RegistryFriendlyByteBuf::new(out.into_inner(), RegistryAccess::empty());
        let decoded = NumberFormat::decode_stream(&mut input).expect("decode format");
        assert_eq!(&decoded, fmt);
        assert_eq!(input.inner().readable_bytes(), 0);
    }

    let obj = Objective::new(
        "kills".to_owned(),
        ObjectiveCriteria::dummy(),
        Component::literal("Kills"),
        RenderType::Hearts,
        true,
        Some(NumberFormat::Fixed(Component::literal("99"))),
    );
    let mut team = PlayerTeam::new("heroes".to_owned());
    team.set_display_name_raw(Component::literal("Heroes"));
    team.set_player_prefix_raw(Some(Component::literal("[H] ")));
    team.set_player_suffix_raw(Some(Component::literal("!")));
    team.set_color_raw(Some(TeamColor::Aqua));
    team.set_allow_friendly_fire_raw(false);
    team.set_name_tag_visibility_raw(Visibility::HideForOwnTeam);
    team.set_collision_rule_raw(CollisionRule::Never);
    team.add_player_raw("Alex".to_owned());
    team.add_player_raw("Steve".to_owned());

    let packets = vec![
        ScoreboardPacket::SetObjective(ClientboundSetObjectivePacket::from_objective(
            &obj,
            ClientboundSetObjectivePacket::METHOD_ADD,
        )),
        ScoreboardPacket::SetObjective(ClientboundSetObjectivePacket::from_objective(
            &obj,
            ClientboundSetObjectivePacket::METHOD_CHANGE,
        )),
        ScoreboardPacket::SetDisplayObjective(ClientboundSetDisplayObjectivePacket::from_name(
            DisplaySlot::Sidebar,
            Some("kills"),
        )),
        ScoreboardPacket::SetDisplayObjective(ClientboundSetDisplayObjectivePacket::from_name(
            DisplaySlot::TeamGreen,
            None,
        )),
        ScoreboardPacket::SetScore(ClientboundSetScorePacket {
            owner: "Alex".to_owned(),
            objective_name: "kills".to_owned(),
            score: -12345,
            display: Some(Component::literal("Sir Alex")),
            number_format: Some(NumberFormat::Styled(
                Style::EMPTY.with_color_format(ChatFormatting::Gold),
            )),
        }),
        ScoreboardPacket::SetScore(ClientboundSetScorePacket {
            owner: "Steve".to_owned(),
            objective_name: "kills".to_owned(),
            score: 0,
            display: None,
            number_format: None,
        }),
        ScoreboardPacket::ResetScore(ClientboundResetScorePacket {
            owner: "Alex".to_owned(),
            objective_name: Some("kills".to_owned()),
        }),
        ScoreboardPacket::ResetScore(ClientboundResetScorePacket {
            owner: "Alex".to_owned(),
            objective_name: None,
        }),
        ScoreboardPacket::SetPlayerTeam(ClientboundSetPlayerTeamPacket::create_add_or_modify(
            &team, true, true,
        )),
        ScoreboardPacket::SetPlayerTeam(ClientboundSetPlayerTeamPacket::create_add_or_modify(
            &team, false, true,
        )),
        ScoreboardPacket::SetPlayerTeam(ClientboundSetPlayerTeamPacket::create_remove("heroes")),
        ScoreboardPacket::SetPlayerTeam(ClientboundSetPlayerTeamPacket::create_player(
            "heroes",
            "Alex",
            TeamPacketAction::Add,
        )),
        ScoreboardPacket::SetPlayerTeam(ClientboundSetPlayerTeamPacket::create_player(
            "heroes",
            "Alex",
            TeamPacketAction::Remove,
        )),
    ];

    for pkt in &packets {
        let decoded = roundtrip_packet(pkt);
        assert_eq!(&decoded, pkt);
    }

    // METHOD_REMOVE omits display_name/render_type/number_format on the wire per Paper.
    let remove_pkt = ScoreboardPacket::SetObjective(ClientboundSetObjectivePacket::from_objective(
        &obj,
        ClientboundSetObjectivePacket::METHOD_REMOVE,
    ));
    let decoded_remove = roundtrip_packet(&remove_pkt);
    assert_eq!(
        decoded_remove,
        ScoreboardPacket::SetObjective(ClientboundSetObjectivePacket {
            objective_name: "kills".to_owned(),
            display_name: Component::empty(),
            render_type: RenderType::Integer,
            number_format: None,
            method: ClientboundSetObjectivePacket::METHOD_REMOVE,
        })
    );

    // Verify exact generated MC 26.2 protocol IDs
    assert_eq!(ClientboundResetScorePacket::protocol_id(), 79);
    assert_eq!(ClientboundSetDisplayObjectivePacket::protocol_id(), 98);
    assert_eq!(ClientboundSetObjectivePacket::protocol_id(), 106);
    assert_eq!(ClientboundSetPlayerTeamPacket::protocol_id(), 109);
    assert_eq!(ClientboundSetScorePacket::protocol_id(), 110);
}

#[test]
fn persistence_nbt_roundtrip_and_dirty_flag() {
    let mut sb = ServerScoreboard::new();
    sb.add_objective(
        "coins",
        ObjectiveCriteria::dummy(),
        Component::literal("Coins"),
        RenderType::Integer,
        true,
        Some(NumberFormat::sidebar_default()),
    )
    .unwrap();
    sb.add_objective(
        "hearts",
        ObjectiveCriteria::health(),
        Component::literal("Hearts"),
        RenderType::Hearts,
        false,
        Some(NumberFormat::Blank),
    )
    .unwrap();

    let alice = ScoreHolder::for_name_only("Alice");
    sb.mutate_player_score(&alice, "coins", false, |access| {
        access.set(500)?;
        access.unlock();
        access.set_display(Some(Component::literal("Rich Alice")));
        access.set_number_format_override(Some(NumberFormat::Fixed(Component::literal("$500"))));
        Ok(())
    })
    .unwrap();
    sb.mutate_player_score(&alice, "hearts", true, |a| a.set(18))
        .unwrap();

    sb.set_display_objective(DisplaySlot::Sidebar, Some("coins"))
        .unwrap();
    sb.set_display_objective(DisplaySlot::BelowName, Some("hearts"))
        .unwrap();

    sb.add_player_team("vip").unwrap();
    sb.set_team_display_name("vip", Component::literal("VIP"))
        .unwrap();
    sb.set_team_prefix("vip", Some(Component::literal("[VIP] ")))
        .unwrap();
    sb.set_team_suffix("vip", Some(Component::literal(" +")))
        .unwrap();
    sb.set_team_color("vip", Some(TeamColor::Gold)).unwrap();
    sb.set_team_allow_friendly_fire("vip", false).unwrap();
    sb.set_team_see_friendly_invisibles("vip", true).unwrap();
    sb.set_team_name_tag_visibility("vip", Visibility::HideForOtherTeams)
        .unwrap();
    sb.set_team_death_message_visibility("vip", Visibility::HideForOwnTeam)
        .unwrap();
    sb.set_team_collision_rule("vip", CollisionRule::PushOtherTeams)
        .unwrap();
    sb.add_player_to_team("Alice", "vip").unwrap();
    sb.add_player_to_team("Zara", "vip").unwrap();

    assert!(sb.is_dirty());
    let save_data = ScoreboardSaveData::from_scoreboard(&sb);
    let tag = save_data.encode_nbt().expect("encode scoreboard NBT");
    sb.clear_dirty();
    assert!(!sb.is_dirty());

    let loaded_data = ScoreboardSaveData::decode_nbt(&tag).expect("decode scoreboard NBT");
    let mut reloaded = ServerScoreboard::new();
    loaded_data
        .apply_to(&mut reloaded)
        .expect("load into scoreboard");

    // Verify objectives
    let coins = reloaded.get_objective("coins").unwrap();
    assert_eq!(coins.display_name(), &Component::literal("Coins"));
    assert!(coins.display_auto_update());
    assert_eq!(
        coins.number_format(),
        Some(&NumberFormat::sidebar_default())
    );

    let hearts = reloaded.get_objective("hearts").unwrap();
    assert_eq!(hearts.render_type(), RenderType::Hearts);
    assert_eq!(hearts.number_format(), Some(&NumberFormat::Blank));

    // Verify scores
    let alice_coins = reloaded.get_player_score_info(&alice, "coins").unwrap();
    assert_eq!(alice_coins.value, 500);
    assert!(!alice_coins.locked);
    assert_eq!(
        alice_coins.number_format,
        Some(NumberFormat::Fixed(Component::literal("$500")))
    );
    let entries = reloaded.list_player_scores("coins");
    assert_eq!(entries[0].display, Some(Component::literal("Rich Alice")));
    assert_eq!(
        reloaded
            .get_player_score_info(&alice, "hearts")
            .map(|s| s.value),
        Some(18)
    );

    // Verify display slots & tracking restored
    assert_eq!(
        reloaded
            .get_display_objective(DisplaySlot::Sidebar)
            .map(Objective::name),
        Some("coins")
    );
    assert_eq!(
        reloaded
            .get_display_objective(DisplaySlot::BelowName)
            .map(Objective::name),
        Some("hearts")
    );
    assert!(reloaded.is_objective_tracked("coins"));
    assert!(reloaded.is_objective_tracked("hearts"));

    // Verify team & ordered members
    let vip = reloaded.get_player_team("vip").unwrap();
    assert_eq!(vip.display_name(), &Component::literal("VIP"));
    assert_eq!(vip.player_prefix(), &Component::literal("[VIP] "));
    assert_eq!(vip.player_suffix(), &Component::literal(" +"));
    assert_eq!(vip.color(), Some(TeamColor::Gold));
    assert!(!vip.is_allow_friendly_fire());
    assert!(vip.can_see_friendly_invisibles());
    assert_eq!(vip.name_tag_visibility(), Visibility::HideForOtherTeams);
    assert_eq!(vip.death_message_visibility(), Visibility::HideForOwnTeam);
    assert_eq!(vip.collision_rule(), CollisionRule::PushOtherTeams);
    let members: Vec<&str> = vip.players().iter().map(String::as_str).collect();
    assert_eq!(members, vec!["Alice", "Zara"]);
}

#[test]
fn persistence_fixture_handles_defaults_and_lenient_team_color() {
    // Construct a minimal Paper 26.2 scoreboard NBT compound with omitted optional fields
    // and an invalid TeamColor ("not_a_color") which Paper decodes leniently as None.
    let mut team_tag = CompoundTag::new();
    team_tag.put_string("Name", "minimal");
    team_tag.put_string("TeamColor", "not_a_color");

    let mut teams_list = ListTag::new();
    teams_list.list.push(Tag::Compound(team_tag));

    let mut root = CompoundTag::new();
    root.put("Teams".to_owned(), Tag::List(teams_list));

    let loaded = ScoreboardSaveData::decode_nbt(&Tag::Compound(root))
        .expect("minimal scoreboard NBT decodes");
    let mut sb = ServerScoreboard::new();
    loaded.apply_to(&mut sb).expect("load into scoreboard");

    let team = sb.get_player_team("minimal").unwrap();
    assert_eq!(team.color(), None);
    assert!(team.is_allow_friendly_fire());
    assert!(team.can_see_friendly_invisibles());
    assert_eq!(team.name_tag_visibility(), Visibility::Always);
    assert_eq!(team.death_message_visibility(), Visibility::Always);
    assert_eq!(team.collision_rule(), CollisionRule::Always);
    assert!(team.players().is_empty());
}

#[test]
fn scoreboard_provider_trait_access() {
    struct Holder {
        sb: ServerScoreboard,
    }
    impl ScoreboardProvider for Holder {
        fn scoreboard(&self) -> &ServerScoreboard {
            &self.sb
        }
        fn scoreboard_mut(&mut self) -> &mut ServerScoreboard {
            &mut self.sb
        }
    }

    let mut holder = Holder {
        sb: ServerScoreboard::new(),
    };
    holder
        .scoreboard_mut()
        .add_objective(
            "m4_test",
            ObjectiveCriteria::dummy(),
            Component::literal("M4"),
            RenderType::Integer,
            false,
            None,
        )
        .unwrap();
    assert!(holder.scoreboard().get_objective("m4_test").is_some());
}
