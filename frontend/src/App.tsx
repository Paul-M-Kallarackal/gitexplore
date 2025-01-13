import { useState } from 'react';
import { GithubIcon, Star, Users, Code2} from 'lucide-react';
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card } from "@/components/ui/card";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { fetchGitHubUser, callLLM } from './actions';
import { UserDetails } from './UserDetails';


interface ChatMessage {
  role: 'user' | 'assistant';
  content: string;
}

interface GitHubUser {
  login: string;
}

interface GitHubRepository {
    name: string;
    description:string;
    size:string;
    full_name:string;
}

function App() {
  const [githubId, setGithubId] = useState('');
  const [hasSearched, setHasSearched] = useState(false);
  const [selectedRepo, setSelectedRepo] = useState<GitHubRepository | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [newMessage, setNewMessage] = useState('');
  const [followers, setFollowers] = useState<GitHubUser[]>([]);
  const [following, setFollowing] = useState<GitHubUser[]>([]);
  const [repos, setRepos] = useState<GitHubRepository[]>([]);
  const [starredRepos, setStarredRepos] = useState<GitHubRepository[]>([]);
  const [loading, setLoading] = useState(false);


  const handleSearch = async (githubId: string) => {
    if (githubId.trim()) {
      try {
        const userConnections = await fetchGitHubUser(githubId);

        setFollowers(userConnections.data.userDetails.followers);
        setFollowing(userConnections.data.userDetails.following);
        setRepos(userConnections.data.userDetails.repositories);
        setStarredRepos(userConnections.data.userDetails.starredRepositories);
  
        setHasSearched(true);
      } catch (err) {
        console.error("Error fetching data:", err);
      }
    }
  };

  const handleBack = () => {
    setHasSearched(false);
    setGithubId('');
  };

  const handleSendMessage = async () => {
    setLoading(true);
    if (newMessage.trim()) {
      if (selectedRepo) {
        const url = "https://github.com/" + selectedRepo.full_name;
      const answer = await callLLM(url, newMessage);
      console.log(answer);
      setMessages([
        ...messages,
        { role: 'user', content: newMessage },
        { role: 'assistant', content: answer }
      ]);
    }
      setLoading(false);
      setNewMessage('');
    }
  };

  if (hasSearched) {
    return (
      <div className="min-h-screen bg-background">
        <UserDetails
          githubId={githubId}
          onBack={handleBack}
          onUserClick={(newUserId) => 
            {
              setGithubId(newUserId)
              handleSearch(newUserId)
            }}
          followers={followers}
          following={following}
          repos={repos}
          starredRepos={starredRepos}
          setChatRepo={(repo) =>
            setSelectedRepo(repo)
          }
        />
        <Dialog open={!!selectedRepo} onOpenChange={() => {setSelectedRepo(null); setLoading(false); setMessages([])}}>
          <DialogContent className="max-w-xl h-[60vh] bg-background">
              <DialogTitle className="text-lg font-semibold">
                Talk to {selectedRepo?.name}
              </DialogTitle>
            <div className="flex flex-col h-full">  
              <ScrollArea className="flex-1 pr-4">
          {messages.map((message, index) => (
            <div
              key={index}
              className={`mb-4 ${
                message.role === 'user' ? 'text-right' : 'text-left'
              }`}
            >
              <div
                className={`inline-block p-3 rounded-lg ${
            message.role === 'user'
              ? 'bg-primary text-primary-foreground'
              : 'bg-muted'
                }`}
              >
                {message.content}
              </div>
            </div>
          ))}
              {loading && (
                <div className="flex justify-center">
                  <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"></div>
                </div>
              )}
              </ScrollArea>
              <div className="flex items-center gap-2 pt-4">
                <Input
                  placeholder="Ask about this repository..."
                  value={newMessage}
                  onChange={(e) => setNewMessage(e.target.value)}
                  onKeyDown={(e) => e.key === 'Enter' && handleSendMessage()}
                />
              <Button onClick={handleSendMessage} disabled={loading}>Send</Button>
              </div>
            </div>
          </DialogContent>
        </Dialog>
        </div>
      );
  }

  return (
    <div className="min-h-screen bg-background">
      {/* Navbar */}
      <nav className="border-b">
        <div className="container mx-auto px-4">
          <div className="flex h-16 items-center justify-between">
            <div className="flex items-center space-x-2">
              <GithubIcon className="h-6 w-6" />
              <span className="text-xl font-bold">GitExplore</span>
            </div>
          </div>
        </div>
      </nav>

      {/* Hero Section */}
      <section className="py-20">
        <div className="container mx-auto px-4">
          <div className="text-center max-w-3xl mx-auto">
            <h1 className="text-4xl font-bold mb-6">
              Explore GitHub Repositories with KGAI
            </h1>
            <p className="text-xl text-muted-foreground mb-8">
              Transform your GitHub experience with AI-powered repository insights. Get instant answers about your code, documentation, and more.
            </p>
            <div className="flex w-full max-w-sm mx-auto items-center space-x-2">
              <Input
                placeholder="Enter GitHub username"
                value={githubId}
                onChange={(e) => setGithubId(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleSearch(githubId)}
              />
              <Button onClick={() => handleSearch(githubId)}>Explore</Button>
            </div>
          </div>
        </div>
      </section>

      {/* Features */}
      <section className="py-40 bg-secondary/50">
        <div className="container mx-auto px-4">
          <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
            <Card className="p-6">
              <Code2 className="h-12 w-12 mb-4 text-primary" />
              <h3 className="text-xl font-semibold mb-2">Smart Code Analysis</h3>
              <p className="text-muted-foreground">Get instant insights about your codebase with AI-powered analysis.</p>
            </Card>
            <Card className="p-6">
              <Users className="h-12 w-12 mb-4 text-primary" />
              <h3 className="text-xl font-semibold mb-2">Team Collaboration</h3>
              <p className="text-muted-foreground">Enhance team productivity with shared repository knowledge and Easier Network Access.</p>
            </Card>
            <Card className="p-6">
              <Star className="h-12 w-12 mb-4 text-primary" />
              <h3 className="text-xl font-semibold mb-2">Repository Insights</h3>
              <p className="text-muted-foreground">Understand your project's structure and dependencies better.</p>
            </Card>
          </div>
        </div>
      </section>
    </div>
  );
}

export default App;