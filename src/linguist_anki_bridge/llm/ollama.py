import json
import requests
from rich.console import Console
from rich.progress import Progress, BarColumn, TextColumn, TimeRemainingColumn, DownloadColumn
from linguist_anki_bridge.llm.base import LLMProvider
from linguist_anki_bridge.config import settings

def check_and_pull_model(ollama_url: str, model_name: str, console: Console) -> None:
    """Checks if a model is installed in Ollama; if not, pulls it with a progress bar."""
    try:
        res = requests.get(f"{ollama_url}/api/tags")
        res.raise_for_status()
        models_data = res.json()
        available_models = [m["name"] for m in models_data.get("models", [])]
        
        # Match model name either directly or without the tag
        matched = False
        for m in available_models:
            if m == model_name or m.split(":")[0] == model_name.split(":")[0]:
                matched = True
                break
        if matched:
            return
    except Exception as e:
        console.print(f"[yellow]Ollama offline or model check failed: {e}[/yellow]")
        return

    console.print(f"[cyan]Model '{model_name}' not found locally. Pulling from Ollama registry...[/cyan]")
    try:
        payload = {"name": model_name}
        with requests.post(f"{ollama_url}/api/pull", json=payload, stream=True) as r:
            r.raise_for_status()
            
            with Progress(
                TextColumn("[bold blue]{task.description}"),
                BarColumn(),
                DownloadColumn(),
                TimeRemainingColumn(),
                console=console
            ) as progress:
                task_id = progress.add_task(f"Downloading {model_name}...", total=100)
                
                for line in r.iter_lines():
                    if line:
                        chunk = json.loads(line.decode('utf-8'))
                        status = chunk.get("status", "")
                        completed = chunk.get("completed", 0)
                        total = chunk.get("total", 0)
                        
                        if total > 0:
                            progress.update(task_id, completed=completed, total=total, description=f"{status}")
                        else:
                            progress.update(task_id, description=f"{status}")
        console.print(f"[green]Successfully pulled model '{model_name}'![/green]")
    except Exception as e:
        console.print(f"[red]Failed to pull model '{model_name}': {e}[/red]")

class OllamaProvider(LLMProvider):
    def __init__(self):
        self.url = settings.ollama_url.rstrip("/")
        self.model_name = settings.ollama_model
        self.vision_model_name = settings.ollama_vision_model
        self.console = Console()
        
    def _ensure_model(self, model: str) -> None:
        check_and_pull_model(self.url, model, self.console)

    def generate_text(self, prompt: str) -> str:
        self._ensure_model(self.model_name)
        payload = {
            "model": self.model_name,
            "prompt": prompt,
            "stream": False,
            "options": {
                "temperature": 0.3
            },
            "format": "json"  # Instructs Ollama to enforce structured JSON output
        }
        
        try:
            res = requests.post(f"{self.url}/api/generate", json=payload)
            res.raise_for_status()
            return res.json().get("response", "")
        except Exception as e:
            raise RuntimeError(f"Ollama text generation failed: {e}")

    def generate_from_image(self, prompt: str, image_base64: str) -> str:
        self._ensure_model(self.vision_model_name)
        payload = {
            "model": self.vision_model_name,
            "prompt": prompt,
            "images": [image_base64],
            "stream": False,
            "options": {
                "temperature": 0.2
            },
            "format": "json"  # Instructs Ollama to enforce structured JSON output
        }
        
        try:
            res = requests.post(f"{self.url}/api/generate", json=payload)
            res.raise_for_status()
            return res.json().get("response", "")
        except Exception as e:
            raise RuntimeError(f"Ollama vision generation failed: {e}")
