import type { UserCommitRepositoryInsights } from '@gitexplore/api-client';
import { Link } from 'react-router-dom';
import { Badge, Heading, Text } from 'strawn';
import { ClockIcon, GitHubIcon } from 'strawn-icons';

import { cacheLabel, compactNumber, formatTimestamp } from '../lib/format';
import { buildRepositoryHref, type ConnectionDirection } from '../lib/graph-navigation';

export function UserInsights({
  insight,
  trail,
  direction,
}: {
  insight: UserCommitRepositoryInsights;
  trail: string[];
  direction: ConnectionDirection;
}) {
  return (
    <section className="commit-signal" aria-labelledby="commit-signal-title">
      <div className="section-heading-row">
        <div>
          <Text size="xs" color="$mutedForeground">Recent public work</Text>
          <Heading id="commit-signal-title" size="h2">Where @{insight.login} is pushing</Heading>
        </div>
        <Badge tone={insight.sourceTruncated ? 'warning' : 'neutral'}>{cacheLabel(insight.cacheStatus)}</Badge>
      </div>
      {insight.repositories.length ? (
        <ol className="commit-list">
          {insight.repositories.map((repository) => (
            <li key={repository.githubId || repository.fullName}>
              <Link to={buildRepositoryHref(repository.fullName, trail, direction)}>
                <GitHubIcon aria-hidden="true" size={16} />
                <span>
                  <strong>{repository.fullName}</strong>
                  <small>{compactNumber(repository.commitCount)} commits · {compactNumber(repository.pushCount)} pushes</small>
                </span>
                <time dateTime={repository.lastPushedAt}>
                  <ClockIcon aria-hidden="true" size={13} />
                  {formatTimestamp(repository.lastPushedAt)}
                </time>
              </Link>
            </li>
          ))}
        </ol>
      ) : <Text size="sm" color="$mutedForeground">No public push events were found in this {insight.windowDays}-day window.</Text>}
      <Text size="xs" color="$mutedForeground">{insight.sourceDescription}</Text>
    </section>
  );
}
