
import { dgraph } from "@hypermode/modus-sdk-as"
import { JSON } from "json-as"
import { Repository, GetRepositoryResponse, Person, GetPersonResponse } from "./classes"
import { connection } from "./constants"

export function getRepository(name: string): Repository {
  const statement = `  
  query getRepository($name: string) {
    getRepository(func: eq(Repository.name, $name))  {
      Repository.description
      Repository.name
      Repository.size
    }
  }`

  const vars = new dgraph.Variables()
  vars.set("$name", name)

  const resp = dgraph.execute(
    connection,
    new dgraph.Request(new dgraph.Query(statement,vars)),
  )
  const persons = JSON.parse<GetRepositoryResponse>(resp.Json).getRepository
  return persons[0]
}

export function getPerson(name: string): Person {
  const statement = ` 
  query getPerson($name: string) {
    persons(func: eq(Person.name, $name))  {
        Person.name
        Person.age
    }
  }`

  const vars = new dgraph.Variables()
  vars.set("$name", name)

  const resp = dgraph.execute(
    connection,
    new dgraph.Request(new dgraph.Query(statement, vars)),
  )
  const persons = JSON.parse<GetPersonResponse>(resp.Json).persons
  return persons[0]
}