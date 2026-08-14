package io.papermc.paper.world.worldgen;

import com.mojang.serialization.Codec;
import com.mojang.serialization.MapCodec;
import com.mojang.serialization.codecs.RecordCodecBuilder;
import net.minecraft.core.Registry;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.util.Mth;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.levelgen.PositionalRandomFactory;
import net.minecraft.world.level.levelgen.SurfaceRules;
import net.minecraft.world.level.levelgen.VerticalAnchor;

/**
 * Probe-local shadow of Paper's {@code OptionallyFlatBedrockConditionSource}
 * (pinned 26.2-DEV-main@0a99345). The real class reads
 * {@code ruleContext.context.level()} to test
 * {@code paperConfig().environment.generateFlatBedrock}, which throws when the
 * surface is built with a Level-free {@code WorldGenerationContext}. This
 * buildSurface oracle drives the generator without a Level, so this shadow
 * substitutes the DEFAULT config path (generateFlatBedrock = false) and is
 * registered under the SAME codec id, making the loaded overworld surface
 * rules decode to this class instead of the jar's.
 *
 * The substitution is exact for the default overworld columns captured here:
 * with generateFlatBedrock = false the real apply() never consults isRoof or
 * the flatYLevel term and reduces to the two anchors' resolved Ys plus the
 * gradient/probability logic reproduced below. Worlds that actually enable
 * flat bedrock are out of scope for this oracle.
 */
public record OptionallyFlatBedrockConditionSource(
    Identifier randomName,
    VerticalAnchor trueAtAndBelow,
    VerticalAnchor falseAtAndAbove,
    boolean isRoof
) implements SurfaceRules.ConditionSource {

    private static final ResourceKey<MapCodec<? extends SurfaceRules.ConditionSource>> CODEC_RESOURCE_KEY = ResourceKey.create(
        Registries.MATERIAL_CONDITION,
        Identifier.fromNamespaceAndPath(Identifier.PAPER_NAMESPACE, "optionally_flat_bedrock_condition_source")
    );
    private static final MapCodec<OptionallyFlatBedrockConditionSource> CODEC = RecordCodecBuilder.mapCodec(i -> i.group(
        Identifier.CODEC.fieldOf("random_name").forGetter(OptionallyFlatBedrockConditionSource::randomName),
        VerticalAnchor.CODEC.fieldOf("true_at_and_below").forGetter(OptionallyFlatBedrockConditionSource::trueAtAndBelow),
        VerticalAnchor.CODEC.fieldOf("false_at_and_above").forGetter(OptionallyFlatBedrockConditionSource::falseAtAndAbove),
        Codec.BOOL.fieldOf("is_roof").forGetter(OptionallyFlatBedrockConditionSource::isRoof)
    ).apply(i, OptionallyFlatBedrockConditionSource::new));

    public static void bootstrap() {
        Registry.register(BuiltInRegistries.MATERIAL_CONDITION, CODEC_RESOURCE_KEY, CODEC);
    }

    @Override
    public MapCodec<OptionallyFlatBedrockConditionSource> codec() {
        return CODEC;
    }

    @Override
    public SurfaceRules.Condition apply(final SurfaceRules.Context ruleContext) {
        // Real class: hasFlatBedrock = context.level().paperConfig().environment.generateFlatBedrock.
        // Default false => trueAtAndBelowY = tempTrueAtAndBelowY, falseAtAndAboveY = tempFalseAtAndAboveY.
        final int trueAtAndBelowY = this.trueAtAndBelow().resolveY(ruleContext.context);
        final int falseAtAndAboveY = this.falseAtAndAbove().resolveY(ruleContext.context);

        final PositionalRandomFactory randomFactory = ruleContext.randomState.getOrCreateRandomFactory(this.randomName());

        class VerticalGradientCondition extends SurfaceRules.LazyYCondition {
            private VerticalGradientCondition() {
                super(ruleContext);
            }

            @Override
            protected boolean compute() {
                int blockY = this.context.blockY;
                if (blockY <= trueAtAndBelowY) {
                    return true;
                }
                if (blockY >= falseAtAndAboveY) {
                    return false;
                }
                double probability = Mth.map(blockY, trueAtAndBelowY, falseAtAndAboveY, 1.0, 0.0);
                RandomSource random = randomFactory.at(this.context.blockX, blockY, this.context.blockZ);
                return random.nextFloat() < probability;
            }
        }

        return new VerticalGradientCondition();
    }
}
