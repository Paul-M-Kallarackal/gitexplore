import type { RepositoryCandidate } from '@gitexplore/api-client';
import { Link } from 'react-router-dom';
import { Avatar, Badge, Button, Card, Text } from 'strawn';
import { BookmarkIcon, CheckIcon, GitHubIcon, TrendingUpIcon } from 'strawn-icons';

import { compactNumber } from '../lib/format';
import { buildExploreHref, buildRepositoryHref, type ConnectionDirection } from '../lib/graph-navigation';

export function RepositoryCandidateCard({
  candidate,
  rank,
  trail,
  direction,
  saving,
  error,
  onSave,
}: {
  candidate: RepositoryCandidate;
  rank: number;
  trail: string[];
  direction: ConnectionDirection;
  saving: boolean;
  error?: string;
  onSave: () => void;
}) {
  const repository = candidate.repository;
  const viaLogins = candidate.viaLogins.slice(0, 2);

  return (
    <Card className="repository-result">
      <div className="repo-rank" aria-label={`Ranked discovery ${rank}`}>
        <span>rank</span>
        <strong>{String(rank).padStart(2, '0')}</strong>
      </div>
      <div className="repo-result-main">
        <div className="repo-title-row">
          <div className="repo-identity">
            <Avatar
              src={`https://github.com/${encodeURIComponent(repository.ownerLogin)}.png?size=80`}
              name={repository.ownerLogin}
              size="sm"
            />
            <div>
              <Text size="xs" color="$mutedForeground">{repository.ownerLogin}</Text>
              <h3><Link to={buildRepositoryHref(repository.fullName, trail, direction)}>{repository.name}</Link></h3>
            </div>
          </div>
          {repository.primaryLanguage ? <Badge tone="neutral">{repository.primaryLanguage}</Badge> : null}
        </div>
        <Text size="sm" color="$mutedForeground">{repository.description || 'No description supplied.'}</Text>
        {candidate.reasons.length ? (
          <ul className="repo-reasons" aria-label="Why this repository was ranked">
            {candidate.reasons.slice(0, 2).map((reason) => <li key={reason}>{reason}</li>)}
          </ul>
        ) : null}
        <div className="repo-meta">
          <span><TrendingUpIcon aria-hidden="true" size={14} /> {compactNumber(repository.stargazerCount)} stars</span>
          <span><GitHubIcon aria-hidden="true" size={14} /> {compactNumber(candidate.networkStars)} nearby stars</span>
        </div>
        {viaLogins.length ? (
          <div className="repo-via" aria-label="Found through nearby people">
            <Text size="xs" color="$mutedForeground">Found through</Text>
            <div>
              {viaLogins.map((login) => (
                <Link key={login} to={buildExploreHref(login, trail, direction)}>
                  <Avatar src={`https://github.com/${encodeURIComponent(login)}.png?size=48`} name={login} size="sm" />
                  <span>@{login}</span>
                </Link>
              ))}
            </div>
          </div>
        ) : null}
      </div>
      <div className="repo-save">
        <Button
          variant={candidate.saved ? 'outline' : 'solid'}
          size="sm"
          disabled={candidate.saved}
          loading={saving}
          leftIcon={candidate.saved ? <CheckIcon aria-hidden="true" size={15} /> : <BookmarkIcon aria-hidden="true" size={15} />}
          onClick={onSave}
        >
          {candidate.saved ? 'Saved' : 'Save'}
        </Button>
        {error ? <span className="field-error" role="alert">{error}</span> : null}
      </div>
    </Card>
  );
}
