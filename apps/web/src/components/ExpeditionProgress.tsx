import { Badge, Heading, Progress, Text } from 'strawn';
import { MapPinIcon } from 'strawn-icons';

const expeditionMilestones = [
  { threshold: 0, name: 'Trailhead' },
  { threshold: 1, name: 'Scout' },
  { threshold: 3, name: 'Pathfinder' },
  { threshold: 6, name: 'Cartographer' },
] as const;

export function expeditionStage(depth: number) {
  const safeDepth = Math.max(0, Math.floor(depth));
  let currentIndex = 0;
  expeditionMilestones.forEach((milestone, index) => {
    if (safeDepth >= milestone.threshold) currentIndex = index;
  });
  const current = expeditionMilestones[currentIndex]!;
  const next = expeditionMilestones[currentIndex + 1];
  const progress = next ? safeDepth - current.threshold : 1;
  const progressMaximum = next ? next.threshold - current.threshold : 1;
  return {
    current,
    next,
    progress,
    progressMaximum,
    remaining: next ? Math.max(0, next.threshold - safeDepth) : 0,
  };
}

export function ExpeditionProgress({ trailDepth, repositoryCount }: { trailDepth: number; repositoryCount: number }) {
  const stage = expeditionStage(trailDepth);
  const progressLabel = stage.next
    ? `${stage.progress} of ${stage.progressMaximum} connections toward ${stage.next.name}`
    : 'Cartographer rank reached';

  return (
    <section className="expedition-progress" aria-labelledby="expedition-progress-title">
      <div className="expedition-progress-copy">
        <div className="expedition-progress-heading">
          <div>
            <Text size="xs" color="$mutedForeground">Expedition progress</Text>
            <Heading id="expedition-progress-title" size="h2">{stage.current.name}</Heading>
          </div>
          <Badge tone="neutral" leadingIcon={<MapPinIcon aria-hidden="true" size={14} />}>
            {trailDepth} {trailDepth === 1 ? 'hop' : 'hops'}
          </Badge>
        </div>
        <Text size="sm" color="$mutedForeground">
          {stage.next
            ? `Follow ${stage.remaining} more ${stage.remaining === 1 ? 'connection' : 'connections'} to become a ${stage.next.name}.`
            : 'You have mapped a deep public trail. Keep following people when the repository signal is strong.'}
        </Text>
        <Progress label={progressLabel} value={stage.progress} max={stage.progressMaximum} size="sm" />
        <dl className="expedition-facts">
          <div><dt>Current depth</dt><dd>{trailDepth}</dd></div>
          <div><dt>Repository signals here</dt><dd>{repositoryCount}</dd></div>
        </dl>
      </div>
      <img
        className="expedition-progress-art"
        src="/images/gitexplore-atlas.webp"
        alt=""
        width="1716"
        height="916"
        loading="lazy"
        decoding="async"
      />
    </section>
  );
}
