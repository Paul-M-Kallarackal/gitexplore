import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { Bookmark, ExplorationSnapshot } from '@gitexplore/api-client';
import { type FormEvent, useMemo, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { Accordion, Alert, Badge, Button, CheckboxGroup, Heading, SearchField, SegmentedControl, Skeleton, Tabs, Text, Textarea, TextField } from 'strawn';
import { ArrowRightIcon, BookmarkIcon, CircleAlertIcon, FolderIcon, HistoryIcon, PlusIcon } from 'strawn-icons';

import { api } from '../api';
import { bookmarkKind, describeBookmarkTarget, formatTimestamp, seedLabel, snapshotSummary } from '../lib/format';
import { useOnboarding } from '../onboarding';
import { buildExploreHref, isLikelyGitHubLogin, normalizeLoginInput } from '../lib/graph-navigation';
import { useDocumentTitle } from '../useDocumentTitle';

type SavedView = 'bookmarks' | 'collections' | 'history';
const savedViews = new Set<SavedView>(['bookmarks', 'collections', 'history']);

function bookmarkHref(bookmark: Bookmark) {
  if ('GitHubUser' in bookmark.target) return buildExploreHref(bookmark.target.GitHubUser.login);
  return repositoryHref(bookmark.target.GitHubRepository.full_name) ?? '/app/saved';
}

function repositoryHref(fullName: string) {
  const [owner, repo] = fullName.split('/', 2);
  return owner && repo ? `/app/repository/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}` : null;
}

function snapshotHref(snapshot: ExplorationSnapshot) {
  if ('User' in snapshot.seed) return buildExploreHref(snapshot.seed.User.login);
  if ('Repository' in snapshot.seed) return repositoryHref(snapshot.seed.Repository.full_name);
  return null;
}

function normalizeRepositoryInput(value: string) {
  let normalized = value.trim().replace(/^https?:\/\/(?:www\.)?github\.com\//i, '');
  normalized = normalized.split(/[?#]/, 1)[0] ?? '';
  const [owner, repository] = normalized.replace(/^\/+|\/+$/g, '').split('/', 2);
  return owner && repository ? `${owner}/${repository}` : normalized;
}

export function SavedPage() {
  useDocumentTitle('Saved');
  const [searchParams, setSearchParams] = useSearchParams();
  const requestedView = searchParams.get('view') as SavedView | null;
  const view: SavedView = requestedView && savedViews.has(requestedView) ? requestedView : 'bookmarks';
  const setView = (next: string) => setSearchParams(next === 'bookmarks' ? {} : { view: next }, { replace: true });

  return (
    <div className="page-stack saved-page">
      <header className="page-heading compact">
        <div><Text size="xs" color="$mutedForeground">Private field notes</Text><Heading size="h1">Saved</Heading></div>
        <Text color="$mutedForeground">Repositories, collections, and older exploration snapshots live in one place now.</Text>
      </header>
      <Tabs
        label="Saved content"
        value={view}
        onValueChange={setView}
        items={[
          { value: 'bookmarks', label: 'Bookmarks', content: <BookmarksView /> },
          { value: 'collections', label: 'Collections', content: <CollectionsView /> },
          { value: 'history', label: 'Trail history', content: <HistoryView /> },
        ]}
      />
    </div>
  );
}

function BookmarksView() {
  const queryClient = useQueryClient();
  const { refreshProgress } = useOnboarding();
  const [search, setSearch] = useState('');
  const [targetKind, setTargetKind] = useState<'repository' | 'user'>('repository');
  const [targetValue, setTargetValue] = useState('');
  const [note, setNote] = useState('');
  const [selectedCategories, setSelectedCategories] = useState<string[]>([]);
  const [validationError, setValidationError] = useState('');
  const query = useQuery({ queryKey: ['bookmarks'], queryFn: () => api.getBookmarks(), retry: false });
  const categories = useQuery({ queryKey: ['categories'], queryFn: () => api.getCategories(), retry: false });
  const addBookmark = useMutation({
    mutationFn: (payload: { target: string; kind: 'repository' | 'user'; note: string | null; categories: string[] }) => api.addBookmark({
      target: payload.kind === 'user'
        ? { GitHubUser: { login: payload.target } }
        : { GitHubRepository: { full_name: payload.target } },
      categories: payload.categories,
      note: payload.note,
    }),
    onSuccess: async () => {
      setTargetValue('');
      setNote('');
      setSelectedCategories([]);
      setValidationError('');
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['bookmarks'] }),
        queryClient.invalidateQueries({ queryKey: ['user-neighborhood'] }),
        refreshProgress(),
      ]);
    },
    retry: false,
  });
  const visible = useMemo(() => {
    const term = search.trim().toLowerCase();
    if (!term) return query.data ?? [];
    return (query.data ?? []).filter((bookmark) => [
      describeBookmarkTarget(bookmark.target), bookmark.note ?? '', ...bookmark.categories,
    ].some((value) => value.toLowerCase().includes(term)));
  }, [query.data, search]);

  function submitBookmark(event: FormEvent) {
    event.preventDefault();
    const normalized = targetKind === 'user' ? normalizeLoginInput(targetValue) : normalizeRepositoryInput(targetValue);
    const validRepository = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})\/[A-Za-z0-9._-]{1,100}$/.test(normalized);
    if ((targetKind === 'user' && !isLikelyGitHubLogin(normalized)) || (targetKind === 'repository' && !validRepository)) {
      setValidationError(targetKind === 'user' ? 'Enter a GitHub username or profile URL.' : 'Enter a repository as owner/name or a GitHub repository URL.');
      return;
    }
    setValidationError('');
    addBookmark.mutate({ target: normalized, kind: targetKind, note: note.trim() || null, categories: selectedCategories });
  }

  const addBookmarkForm = (
    <form className="add-bookmark-form" onSubmit={submitBookmark} noValidate>
      <SegmentedControl
        label="Bookmark type"
        value={targetKind}
        onValueChange={(value) => { setTargetKind(value as 'repository' | 'user'); setValidationError(''); }}
        options={[{ label: 'Repository', value: 'repository' }, { label: 'Person', value: 'user' }]}
      />
      <TextField
        label={targetKind === 'repository' ? 'Repository' : 'GitHub username'}
        value={targetValue}
        onChange={(event) => setTargetValue(event.currentTarget.value)}
        placeholder={targetKind === 'repository' ? 'owner/repository' : 'octocat'}
        description={targetKind === 'repository' ? 'Full GitHub URLs work too.' : '@handles and profile URLs work too.'}
        error={validationError || undefined}
        required
      />
      <Textarea label="Note" value={note} onChange={(event) => setNote(event.currentTarget.value)} placeholder="Why is this worth returning to?" rows={3} />
      {categories.data?.length ? (
        <CheckboxGroup
          label="Collections"
          options={categories.data.map((category) => ({ label: category.name, value: category.name }))}
          value={selectedCategories}
          onValueChange={setSelectedCategories}
        />
      ) : null}
      <div className="add-bookmark-actions">
        <Text size="xs" color="$mutedForeground">Bookmarks are private to your GitExplore session.</Text>
        <Button type="submit" loading={addBookmark.isPending} leftIcon={<PlusIcon aria-hidden="true" size={16} />}>Add bookmark</Button>
      </div>
      {categories.isError ? <Alert tone="warning" title="Collections could not be loaded">You can still save this bookmark without a collection.</Alert> : null}
      {addBookmark.isError ? <Alert tone="error" title="Bookmark was not added">{addBookmark.error.message}</Alert> : null}
      {addBookmark.isSuccess ? <span className="visually-hidden" role="status">Bookmark added.</span> : null}
    </form>
  );

  return (
    <section className="saved-view" aria-labelledby="bookmarks-view-title">
      <div className="saved-tools">
        <div><Text size="xs" color="$mutedForeground">Repository notebook</Text><Heading id="bookmarks-view-title" size="h2">Bookmarks</Heading></div>
        <SearchField label="Search bookmarks" value={search} onChange={(event) => setSearch(event.currentTarget.value)} onClear={search ? () => setSearch('') : undefined} placeholder="Repository, person, note, or collection" />
      </div>
      <Accordion
        type="single"
        items={[{ value: 'add-bookmark', title: 'Add a bookmark manually', content: addBookmarkForm }]}
      />
      <p className="result-count" role="status" aria-live="polite">{visible.length} visible bookmark{visible.length === 1 ? '' : 's'}</p>
      {query.isPending ? <div className="saved-list" aria-busy="true"><Skeleton height="5rem" /><Skeleton height="5rem" /></div> : null}
      {query.isError ? <Alert tone="error" title="Saved entries are unavailable" icon={<CircleAlertIcon aria-hidden="true" size={17} />}>{query.error instanceof Error ? query.error.message : 'The request failed.'}</Alert> : null}
      {query.isSuccess && visible.length ? (
        <ul className="saved-list">
          {visible.map((bookmark) => (
            <li key={bookmark.id}>
              <Link to={bookmarkHref(bookmark)}>
                <span className="saved-kind"><BookmarkIcon aria-hidden="true" size={17} /></span>
                <span className="saved-copy">
                  <strong>{describeBookmarkTarget(bookmark.target)}</strong>
                  <small>{bookmark.note || `${bookmarkKind(bookmark.target)} saved ${formatTimestamp(bookmark.created_at)}`}</small>
                  <span className="saved-tags">{bookmark.categories.length ? bookmark.categories.map((category) => <Badge key={category} tone="neutral">{category}</Badge>) : <Badge tone="neutral">Uncategorized</Badge>}</span>
                </span>
                <ArrowRightIcon aria-hidden="true" size={16} />
              </Link>
            </li>
          ))}
        </ul>
      ) : null}
      {query.isSuccess && !visible.length ? <div className="inline-empty"><Text size="sm" color="$mutedForeground">No bookmarks match. Save a repository while exploring and it appears here.</Text></div> : null}
    </section>
  );
}

function CollectionsView() {
  const queryClient = useQueryClient();
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const categories = useQuery({ queryKey: ['categories'], queryFn: () => api.getCategories(), retry: false });
  const createCategory = useMutation({
    mutationFn: (payload: { name: string; description: string | null }) => api.createCategory(payload),
    onSuccess: async () => {
      setName(''); setDescription('');
      await queryClient.invalidateQueries({ queryKey: ['categories'] });
    },
    retry: false,
  });

  function submit(event: FormEvent) {
    event.preventDefault();
    if (name.trim()) createCategory.mutate({ name: name.trim(), description: description.trim() || null });
  }

  return (
    <section className="saved-view collections-view" aria-labelledby="collections-view-title">
      <form className="collection-form" onSubmit={submit}>
        <div><Text size="xs" color="$mutedForeground">Grouping language</Text><Heading id="collections-view-title" size="h2">Create a collection</Heading></div>
        <TextField label="Name" value={name} onChange={(event) => setName(event.currentTarget.value)} placeholder="Underseen compilers" required />
        <Textarea label="Description" value={description} onChange={(event) => setDescription(event.currentTarget.value)} placeholder="What belongs in this collection?" rows={4} />
        <Button type="submit" loading={createCategory.isPending} leftIcon={<PlusIcon aria-hidden="true" size={16} />}>Create collection</Button>
        {createCategory.isError ? <Alert tone="error" title="Collection was not created">{createCategory.error.message}</Alert> : null}
        {createCategory.isSuccess ? <span className="visually-hidden" role="status">Collection created.</span> : null}
      </form>
      <div className="collection-index">
        <div className="section-heading-row"><Heading size="h2">Your collections</Heading>{categories.data ? <Badge tone="neutral">{categories.data.length}</Badge> : null}</div>
        {categories.isPending ? <div className="saved-list" aria-busy="true"><Skeleton height="4.5rem" /><Skeleton height="4.5rem" /></div> : null}
        {categories.isError ? <Alert tone="error" title="Collections are unavailable">{categories.error instanceof Error ? categories.error.message : 'The request failed.'}</Alert> : null}
        {categories.isSuccess && categories.data.length ? (
          <ul className="collection-list">
            {categories.data.map((category) => <li key={category.name}><FolderIcon aria-hidden="true" size={18} /><span><strong>{category.name}</strong><small>{category.description || 'No description yet.'}</small></span></li>)}
          </ul>
        ) : null}
        {categories.isSuccess && !categories.data.length ? <div className="inline-empty"><Text size="sm" color="$mutedForeground">Create a collection to group saved repositories in your own language.</Text></div> : null}
      </div>
    </section>
  );
}

function HistoryView() {
  const [search, setSearch] = useState('');
  const query = useQuery({ queryKey: ['exploration-snapshots'], queryFn: () => api.getExplorationSnapshots(), retry: false });
  const visible = useMemo(() => {
    const term = search.trim().toLowerCase();
    return !term ? query.data ?? [] : (query.data ?? []).filter((snapshot) => seedLabel(snapshot.seed).toLowerCase().includes(term) || snapshot.id.toLowerCase().includes(term));
  }, [query.data, search]);

  return (
    <section className="saved-view" aria-labelledby="history-view-title">
      <div className="saved-tools">
        <div><Text size="xs" color="$mutedForeground">Legacy snapshots</Text><Heading id="history-view-title" size="h2">Trail history</Heading></div>
        <SearchField label="Search trail history" value={search} onChange={(event) => setSearch(event.currentTarget.value)} onClear={search ? () => setSearch('') : undefined} placeholder="Seed or snapshot ID" />
      </div>
      <p className="result-count" role="status" aria-live="polite">{visible.length} visible trail{visible.length === 1 ? '' : 's'}</p>
      {query.isPending ? <div className="saved-list" aria-busy="true"><Skeleton height="5rem" /><Skeleton height="5rem" /></div> : null}
      {query.isError ? <Alert tone="error" title="Trail history is unavailable">{query.error instanceof Error ? query.error.message : 'The request failed.'}</Alert> : null}
      {query.isSuccess && visible.length ? (
        <ol className="history-list">
          {visible.map((snapshot) => <HistorySnapshotEntry key={snapshot.id} snapshot={snapshot} />)}
        </ol>
      ) : null}
      {query.isSuccess && !visible.length ? <div className="inline-empty"><Text size="sm" color="$mutedForeground">No saved exploration snapshots match.</Text></div> : null}
    </section>
  );
}

export function HistorySnapshotEntry({ snapshot }: { snapshot: ExplorationSnapshot }) {
  const href = snapshotHref(snapshot);
  const label = seedLabel(snapshot.seed);

  return (
    <li>
      <article className="history-entry">
        <div className="history-entry-overview">
          <span className="history-mark"><HistoryIcon aria-hidden="true" size={17} /></span>
          <span className="history-entry-copy">
            <strong>{label}</strong>
            <small>{snapshotSummary(snapshot)} · {formatTimestamp(snapshot.generated_at)}</small>
          </span>
          {href ? (
            <Link className="history-seed-link" to={href} aria-label={`Open ${label}`}>
              Open <ArrowRightIcon aria-hidden="true" size={16} />
            </Link>
          ) : null}
        </div>
        <details className="history-inspector">
          <summary>Inspect snapshot</summary>
          <div className="history-inspection-content">
            <dl className="history-snapshot-meta">
              <div>
                <dt>Snapshot ID</dt>
                <dd><code>{snapshot.id}</code></dd>
              </div>
              <div>
                <dt>Generated</dt>
                <dd>{formatTimestamp(snapshot.generated_at)}</dd>
              </div>
            </dl>
            <div className="history-discovery-grid">
              <SnapshotDiscoveryList label="Discovered people" values={snapshot.discovered_people} kind="person" />
              <SnapshotDiscoveryList label="Discovered repositories" values={snapshot.discovered_repositories} kind="repository" />
            </div>
          </div>
        </details>
      </article>
    </li>
  );
}

function SnapshotDiscoveryList({ label, values, kind }: { label: string; values: string[]; kind: 'person' | 'repository' }) {
  return (
    <section className="history-discovery-group">
      <div className="history-discovery-heading">
        <Heading size="h3">{label}</Heading>
        <Badge tone="neutral">{values.length}</Badge>
      </div>
      {values.length ? (
        <ul className="history-discovery-list">
          {values.map((value, index) => {
            const normalizedLogin = kind === 'person' ? normalizeLoginInput(value) : '';
            const href = kind === 'person' && isLikelyGitHubLogin(normalizedLogin)
              ? buildExploreHref(normalizedLogin)
              : kind === 'repository'
                ? repositoryHref(value)
                : null;
            return <li key={`${value}-${index}`}>{href ? <Link to={href}>{kind === 'person' ? `@${normalizedLogin}` : value}</Link> : <span>{value}</span>}</li>;
          })}
        </ul>
      ) : <Text size="xs" color="$mutedForeground">Nothing was recorded in this snapshot.</Text>}
    </section>
  );
}
