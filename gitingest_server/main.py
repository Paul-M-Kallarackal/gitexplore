from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from gitingest import ingest
import uvicorn
import html

app = FastAPI()

# Add CORS middleware
app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)




summary, tree, content = ingest("https://github.com/cyclotruc/gitingest")

@app.get("/")
async def root():
    return {"message": "Welcome to FastAPI"}

@app.get("/analyze")
def get_summary(url: str):
    summary, tree, content = ingest(url)
    cleaned_summary = html.escape(summary)
    cleaned_tree = html.escape(tree)
    cleaned_content = html.escape(content)
    
    content_length = len(cleaned_content)
    print('Content length:', content_length)
    if content_length > 1000:
        cleaned_content = cleaned_content[-1000:]
    
    return {"summary": cleaned_summary, "tree": cleaned_tree, "content": cleaned_content}

if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=3000)