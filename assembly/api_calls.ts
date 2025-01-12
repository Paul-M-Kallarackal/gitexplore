import { http } from "@hypermode/modus-sdk-as"
import { JSON } from "json-as";
// Define the types based on the structure of the response data
@json class User {
    login: string = "";
  }
  
@json class Repo {
    name: string = "";
  }

  export class ConnectionsData {
    followers: User[] = []
    following: User[] = []
  }
  
  export class RepositoriesData {
    repositories: Repo[] = []
    starredRepositories: Repo[] = []
  }

  export class Followers {
    followers: User[] = []
  }

    export class Following {
        following: User[] = []
    }
  
export function getFollowers(name: string): User[] {
    const response = http.fetch(`https://api.github.com/users/${name}/followers`)
    const data = JSON.parse<User[]>(response.json<string>())
    return data
}

export function getFollowing(name: string): User[] {
    const response = http.fetch(`https://api.github.com/users/${name}/following`)
    const data = JSON.parse<User[]>(response.json<string>())
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
