import { http } from "@hypermode/modus-sdk-as"
import { JSON } from "json-as";

// Define the types based on the structure of the response data
@json class GithubUser {
    login: string = "";

  }
  
@json class Repo {
    name: string = "";
    description: string = "";
    size: i32 = 0;
    full_name: string = "";
  }

  export class ConnectionsData {
    followers: GithubUser[] = []
    following: GithubUser[] = []
    repositories: Repo[] = []
    starredRepositories: Repo[] = []
  }
  
  @json export class GitIngestContent {
    input_text: string = "";
    max_file_size: string = "";
    pattern_type: string = "";
    pattern: string = "";
  }
  export class Followers {
    followers: GithubUser[] = []
  }

    export class Following {
        following: GithubUser[] = []
    }
  
export function getFollowers(name: string): GithubUser[] {
  // first check in dgraph, then search and update it using Github API
    const response = http.fetch(`https://api.github.com/users/${name}/followers`)
    const data = JSON.parse<GithubUser[]>(response.json<string>())
    return data
}

export function getFollowing(name: string): GithubUser[] {
    const response = http.fetch(`https://api.github.com/users/${name}/following`)
    const data = JSON.parse<GithubUser[]>(response.json<string>())
    return data
}

export function getRepositories(name: string): Repo[] {
    const response = http.fetch(`https://api.github.com/users/${name}/repos`)
    const data = JSON.parse<Repo[]>(response.json<string>())
    return data
}

export function getStarredRepositories(name: string): Repo[] {
    const response = http.fetch(`https://api.github.com/users/${name}/starred`)
    const data = JSON.parse<Repo[]>(response.json<string>())
    return data
}

export function getContext(url:string): string {

    const response = http.fetch(`http://localhost:3000/analyze?url=${url}`)
    if (!response.ok) {
      console.log(response.statusText);
    }

    console.log(response.json<string>())
    return response.json<string>()
  }

