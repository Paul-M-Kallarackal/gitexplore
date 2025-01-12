@json
export class Repository {
  @alias("Repository.name")
  name: string = ""
  @alias("Repository.description")
  description: string = ""
  @alias("Repository.size")
  size: i32 = 0
}

@json
export class GetRepositoryResponse {
  getRepository: Repository[] = []
}

@json
export class Person {
  @alias("Person.name")
  name: string=""
  @alias("Person.age")
  age: i32=0
  }

@json
export class GetPersonResponse {
  persons: Person[] = []
}
