# Embedding component

This repository contains a Spin component, which you can use to generate embeddings for texts, and compare against.

The component is general-purpose in that it accepts text you want to store in the database. You can update and delete text in the database, e.g. from a web hook, or by different types of automation.

You can also call the component to compare a given text string with what's in the database already. The component will return a sorted list of text already in the database.

This is built ion the Serverless AI features in Spin and Fermyon Cloud.

The repo contains a small client front-end to try the functionality.

Make sure to run the `./dev/db_schema.sql` to create the required schema in the database. I.e. `spin up --sqlite @dev/db_schema.sql`

## API

### POST “/”

1. Accepts the below array of embeddings and model-stuff as a body
2. Creates the embeddings and stores them in the database
3. Returns OK or ERROR

Data model

```json
{
	"embeddings": [
		{
			"reference": "page-title.md", // Required: A required unique identifier: This is an identifier of the text, e.g., the file name or URI of the text on a web site
			"text": "The text to compare against", // Required: Text - could be a heading
		}
	]
}
```

### GET “/”

If no body, return what’s in the database.

1. Accepts the below data structure
2. Generates an embedding for the provided text
3. Returns similar objects from the database, given the options provided

Data model

```json
{
	"text": "This is a title", // Required: Text to use for comparison
}
```

Returns

```json
{
	"result": [
		{
			"reference": "Doc A",
			"text": "Text",
            		"similarity": 0.454 // 1 is absolute similarity
		}
	]
}
```

### DELETE “/:id”

Takes no body, but deletes an embedding from the database, based on the id in the database


## Short video

<div>
    <a href="https://www.loom.com/share/47c883b8d48a4efa81565895b401beb1">
      <img style="max-width:300px;" src="https://cdn.loom.com/sessions/thumbnails/47c883b8d48a4efa81565895b401beb1-with-play.gif">
    </a>
  </div>
