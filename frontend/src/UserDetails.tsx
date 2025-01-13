import { Card } from "@/components/ui/card";
import { ArrowLeft } from 'lucide-react';
interface UserDetailsProps {
  githubId: string;
  onBack: () => void;
  onUserClick: (newUserId: string) => void; // Function to set new GitHub ID
  followers: { login: string }[];
  following: { login: string }[];
  repos: { name: string, description:string, size:string, full_name:string }[];
  starredRepos: { name: string, description:string, size:string, full_name:string }[];
  setChatRepo: (repo : { name: string, description:string, size:string, full_name:string }) => void;
}

export function UserDetails({ githubId, onBack, onUserClick, followers, following, repos, starredRepos, setChatRepo }: UserDetailsProps) {
  return (
    <div className="min-h-screen bg-background">
      <div className="container mx-auto py-8 px-4">
        <div className="flex items-center gap-4 mb-6">
          <div onClick={onBack}>
            <ArrowLeft />
          </div>
          <h2 className="text-2xl font-bold">GitHub: {githubId}</h2>
        </div>

        {/* Followers Section */}
        <h3 className="text-xl font-semibold mb-4">Followers</h3>
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {followers.map((user) => (
            <Card
              key={user.login}
              className="p-4 hover:shadow-lg transition-shadow cursor-pointer"
              onClick={() => onUserClick(user.login)} // Call parent function with new user ID
            >
              <h4 className="font-bold">{user.login}</h4>
            </Card>
          ))}
        </div>

        {/* Following Section */}
        <h3 className="text-xl font-semibold mt-8 mb-4">Following</h3>
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {following.map((user) => (
            <Card
              key={user.login}
              className="p-4 hover:shadow-lg transition-shadow cursor-pointer"
              onClick={() => onUserClick(user.login)} // Call parent function with new user ID
            >
              <h4 className="font-bold">{user.login}</h4>
            </Card>
          ))}
        </div>

        {/* Repositories Section */}
      <h3 className="text-xl font-semibold mt-8 mb-4">Repositories</h3>
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {repos.map((repo) => (
          <Card
            key={repo.name}
            className="p-6 hover:shadow-lg transition-shadow cursor-pointer"
            onClick={() => setChatRepo(repo)}
          >
            <h4 className="font-bold">{repo.name}</h4>
            <h5 className="text-sm text-muted-foreground">Description : {repo.description !== "ul" ? "" : repo.description}</h5>
            <h5 className="text-sm text-muted-foreground">Size : {repo.size}</h5>
          </Card>
        ))}
      </div>
      <h3 className="text-xl font-semibold mt-8 mb-4">Starred Repositories</h3>
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {starredRepos.map((repo) => (
            <Card
            key={repo.name}
            className="p-6 hover:shadow-lg transition-shadow cursor-pointer"
            onClick={() => setChatRepo(repo)}
          >
            <h4 className="font-bold">{repo.name}</h4>
            <h5 className="text-sm text-muted-foreground">Description : {repo.description !== "ul" ? "" : repo.description}</h5>
            <h5 className="text-sm text-muted-foreground">Size : {repo.size}</h5>
            </Card>
        ))}
      </div>
      </div>
    </div>
  );
}
