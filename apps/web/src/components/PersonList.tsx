import type { GraphUser } from '@gitexplore/api-client';
import { useState } from 'react';
import { Link } from 'react-router-dom';
import { Avatar, Button, Text } from 'strawn';
import { ArrowRightIcon } from 'strawn-icons';

import { buildExploreHref } from '../lib/graph-navigation';
import { compactNumber } from '../lib/format';

export function PersonList({ people, trail, direction }: { people: GraphUser[]; trail: string[]; direction: 'followers' | 'following' }) {
  const [expanded, setExpanded] = useState(false);
  const visible = expanded ? people : people.slice(0, 12);

  if (!people.length) {
    return <div className="lane-empty"><Text size="sm" color="$mutedForeground">No cached {direction} yet.</Text></div>;
  }

  return (
    <div className="person-lane">
      <ul>
        {visible.map((person) => (
          <li key={person.githubId || person.login}>
            <Link to={buildExploreHref(person.login, trail, direction)}>
              <Avatar src={person.avatarUrl ?? undefined} name={person.name || person.login} size="sm" />
              <span className="person-copy">
                <strong>{person.name || person.login}</strong>
                <small>@{person.login} · {compactNumber(person.followersCount)} followers</small>
              </span>
              <ArrowRightIcon aria-hidden="true" size={15} />
            </Link>
          </li>
        ))}
      </ul>
      {people.length > 12 ? (
        <Button variant="ghost" size="sm" onClick={() => setExpanded((value) => !value)}>
          {expanded ? 'Show fewer' : `Show ${people.length - 12} more`}
        </Button>
      ) : null}
    </div>
  );
}
