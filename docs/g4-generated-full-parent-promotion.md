# G4 generated FULL promotion integration

G4 must treat `FULL` as a scheduler/status boundary and a consuming representation change, not as another borrowed `ProtoChunk` executor wave.

## Required flow

1. Run generation through the parent `SPAWN` status, including lighting and the SPAWN work, using the normal status pyramid.
2. Do not dispatch a borrowed `ChunkStatusTask::Full` over `&mut ProtoChunk`. The generic world-generation executor must refuse that task without stamping `FULL` or mutating the proto.
3. Once the holder has the exact SPAWN parent, consume the holder transactionally and call `LevelChunk::from_generated_spawn_proto(proto)` (through `GenerationChunkHolder::into_level_chunk`). This moves the proto into the concrete `LevelChunk`, whose persisted status is `FULL`.
4. Install the returned chunk in `ChunkMap` only after conversion returns `Ok`. A status mismatch, unsupported persisted light state, or palette conversion failure must produce a typed error and no install or replacement.
5. Preserve the existing exact-position and replacement semantics of the install operation. The consuming path must not clone or leave a recoverable proto holder on either success or failure.

The final FULL executor wave should therefore be removed from G4's batch execution loop. The batch should complete all SPAWN/light work first, then perform the consuming promotions as the final transaction for the corresponding holders. If any promotion fails, retain the existing all-target conversion-before-install behavior: do not partially install the batch.

## Data boundary

The promotion already transfers the representations Rivet models on the proto/base: sections, heightmaps, light nibbles and light-correct state, inhabited time, unsaved state, pending block-entity NBT, post-processing offsets, block and fluid ticks, and typed structure starts/references. Serialized entity NBT is carried as `post_load_entities` until ServerLevel authority can run Paper's `postLoadProtoChunk` callback.

G4 must perform the remaining ServerLevel-owned Paper actions only where their runtime authorities exist: post-load processing, block-entity registration, tick-container registration, loaded/full-status callbacks, and the unsaved listener. Do not invent registration or spawn behavior in the value-layer conversion; keep those boundaries issue-linked (`RivetTodo(#185)`) until the corresponding ServerLevel units are present.
