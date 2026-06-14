import json
import re
from typing import Callable, List, Dict, Any

class ValidationError(Exception):
    pass

def extract_and_validate_json(raw_text: str, expected_keys: List[str]) -> Dict[str, Any]:
    """Extracts JSON object from model's response and verifies all expected keys are present."""
    # Find the outer-most curly braces to extract JSON
    json_match = re.search(r'(\{.*\})', raw_text, re.DOTALL)
    if not json_match:
         raise ValidationError("No JSON object found in response. Ensure you return ONLY JSON in `{ ... }` structure.")
         
    json_str = json_match.group(1).strip()
    try:
        data = json.loads(json_str)
    except json.JSONDecodeError as e:
        raise ValidationError(f"Failed to parse JSON content. Error: {e}. Raw content retrieved was:\n{json_str}")
        
    missing_keys = [k for k in expected_keys if k not in data]
    if missing_keys:
        raise ValidationError(f"JSON is missing required keys: {missing_keys}. Current keys found: {list(data.keys())}")
        
    return data

def execute_with_retry(
    llm_call_fn: Callable[[str], str], 
    initial_prompt: str, 
    expected_keys: List[str], 
    max_retries: int = 3
) -> Dict[str, Any]:
    """Executes LLM call with retry loop on JSON validation failure."""
    prompt = initial_prompt
    last_error = None
    
    for attempt in range(max_retries):
        try:
            raw_response = llm_call_fn(prompt)
            return extract_and_validate_json(raw_response, expected_keys)
        except ValidationError as e:
            last_error = e
            # Mutate the prompt for the retry, explaining what went wrong
            prompt = (
                f"{initial_prompt}\n\n"
                f"--- CORRECTION NOTICE (Attempt {attempt + 1} of {max_retries} failed) ---\n"
                f"Your previous attempt produced a JSON validation error: {e}\n"
                f"Fix the output. Return ONLY the valid JSON structure matching the schema. No markdown ticks, no commentary."
            )
            
    raise ValidationError(f"Failed after {max_retries} attempts. Last validation error: {last_error}")
