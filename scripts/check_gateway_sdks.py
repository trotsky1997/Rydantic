"""Verify gateway wire formats with official SDKs and loopback-only fixtures."""
import json, threading, subprocess, os, time, tempfile
from pathlib import Path
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler
import openai, anthropic
ROOT = Path(__file__).resolve().parents[1]
SEEN = []

def frame(value, event=None):
    return (('event: ' + event + '\n' if event else '') + 'data: ' + json.dumps(value, ensure_ascii=False) + '\n\n').encode()

class Handler(BaseHTTPRequestHandler):

    def log_message(self, *args):
        pass

    def do_POST(self):
        req = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        source = {'/v1/responses': 'responses', '/v1/chat/completions': 'chat_completions', '/v1/messages': 'anthropic_messages'}[self.path]
        SEEN.append((source, req))
        assert self.headers.get('x-api-key' if source == 'anthropic_messages' else 'Authorization') == ('fixture-upstream-key' if source == 'anthropic_messages' else 'Bearer fixture-upstream-key')
        tool = bool(req.get('tools')) and 'function_call_output' not in json.dumps(req.get('input', [])) and ('tool_result' not in json.dumps(req.get('messages', []))) and (not any((m.get('role') == 'tool' for m in req.get('messages', []))))
        text = '' if tool else 'SDK hello'
        args = '{"a":19,"b":23}'
        if source == 'responses':
            output = [{'id': 'm1', 'type': 'message', 'role': 'assistant', 'content': [{'type': 'output_text', 'text': text, 'annotations': []}]}]
            if tool:
                output.append({'id': 'fc1', 'type': 'function_call', 'call_id': 'call-sdk', 'name': 'add', 'arguments': args})
            raw = {'id': 'r1', 'model': 'test-model', 'object': 'response', 'status': 'completed', 'output': output, 'usage': {'input_tokens': 3, 'output_tokens': 5}}
            wire = frame({'type': 'response.created', 'response': {'id': 'r1', 'status': 'in_progress', 'output': []}})
            if text:
                wire += frame({'type': 'response.output_text.delta', 'item_id': 'm1', 'output_index': 0, 'content_index': 0, 'delta': text})
            if tool:
                start = dict(output[-1], arguments='')
                wire += frame({'type': 'response.output_item.added', 'output_index': 1, 'item': start})
                wire += frame({'type': 'response.function_call_arguments.delta', 'output_index': 1, 'item_id': 'fc1', 'delta': args})
            wire += frame({'type': 'response.completed', 'response': raw})
        elif source == 'chat_completions':

            def chunk(delta, finish=None):
                return frame({'id': 'c1', 'object': 'chat.completion.chunk', 'created': int(time.time()), 'model': 'test-model', 'choices': [{'index': 0, 'delta': delta, 'finish_reason': finish}]})
            wire = chunk({'role': 'assistant'})
            if text:
                wire += chunk({'content': text})
            if tool:
                wire += chunk({'tool_calls': [{'index': 0, 'id': 'call-sdk', 'type': 'function', 'function': {'name': 'add', 'arguments': args}}]})
            wire += chunk({}, 'tool_calls' if tool else 'stop')
            wire += frame({'id': 'c1', 'model': 'test-model', 'choices': [], 'usage': {'prompt_tokens': 3, 'completion_tokens': 5, 'total_tokens': 8}}) + b'data: [DONE]\n\n'
        else:
            raw = {'id': 'a1', 'type': 'message', 'role': 'assistant', 'model': 'test-model', 'content': [], 'stop_reason': None, 'stop_sequence': None, 'usage': {'input_tokens': 3, 'output_tokens': 0}}
            wire = frame({'type': 'message_start', 'message': raw}, 'message_start')
            block = {'type': 'tool_use', 'id': 'call-sdk', 'name': 'add', 'input': {}} if tool else {'type': 'text', 'text': ''}
            wire += frame({'type': 'content_block_start', 'index': 0, 'content_block': block}, 'content_block_start')
            delta = {'type': 'input_json_delta', 'partial_json': args} if tool else {'type': 'text_delta', 'text': text}
            wire += frame({'type': 'content_block_delta', 'index': 0, 'delta': delta}, 'content_block_delta')
            wire += frame({'type': 'content_block_stop', 'index': 0}, 'content_block_stop')
            wire += frame({'type': 'message_delta', 'delta': {'stop_reason': 'tool_use' if tool else 'end_turn', 'stop_sequence': None}, 'usage': {'output_tokens': 5}}, 'message_delta')
            wire += frame({'type': 'message_stop'}, 'message_stop')
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(wire)))
        self.end_headers()
        self.wfile.write(wire)

PNG = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC'
IMAGE_URL = 'https://example.com/fixture.png?signature=opaque-test-reference'

def image_content(api, kind):
    reference = IMAGE_URL if kind == 'url' else 'data:image/png;base64,' + PNG
    if api == 'responses':
        image = {'type': 'input_image', 'image_url': reference, 'detail': 'auto'}
        text_type = 'input_text'
    elif api == 'chat_completions':
        image = {'type': 'image_url', 'image_url': {'url': reference, 'detail': 'auto'}}
        text_type = 'text'
    else:
        source = {'type': 'url', 'url': IMAGE_URL} if kind == 'url' else {'type': 'base64', 'media_type': 'image/png', 'data': PNG}
        image = {'type': 'image', 'source': source}
        text_type = 'text'
    return [{'type': text_type, 'text': 'SDK image before'}, image, {'type': text_type, 'text': 'SDK image after'}]

def main():
    with tempfile.TemporaryDirectory(prefix='rydantic-gateway-sdk-') as directory:
        WORK = Path(directory)
        upstream = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        threading.Thread(target=upstream.serve_forever, daemon=True).start()
        base = f'http://127.0.0.1:{upstream.server_port}/v1'
        apis = ['responses', 'chat_completions', 'anthropic_messages']
        config = {'listen': '127.0.0.1:0', 'routes': {a: [{'api': a, 'base_url': base, 'model': 'test-model', 'api_key_env': 'SDK_UPSTREAM_KEY'}] for a in apis}, 'clients': [{'id': 'sdk', 'key_env': 'SDK_GATEWAY_KEY', 'models': apis}]}
        path = WORK / 'gateway-sdk-smoke-config.json'
        path.write_text(json.dumps(config))
        env = dict(os.environ, SDK_UPSTREAM_KEY='fixture-upstream-key', SDK_GATEWAY_KEY='fixture-gateway-key-12345')
        p = subprocess.Popen([str(ROOT / 'target/debug/rydantic-ai-gateway'), str(path)], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        checks = []
        try:
            line = p.stdout.readline().strip()
            assert line.startswith('RydanticAI gateway listening on '), line
            gateway = 'http://' + line.split()[-1] + '/v1'
            oa = openai.OpenAI(base_url=gateway, api_key='fixture-gateway-key-12345', max_retries=0, http_client=openai.DefaultHttpxClient(trust_env=False), _strict_response_validation=True)
            an = anthropic.Anthropic(base_url=gateway.removesuffix('/v1'), api_key='fixture-gateway-key-12345', max_retries=0, http_client=anthropic.DefaultHttpxClient(trust_env=False), _strict_response_validation=True)
            for source in apis:
                r = oa.responses.create(model=source, input='hello')
                assert r.output_text == 'SDK hello'
                assert r.model == source
                assert r.usage.total_tokens == 8
                checks.append(['responses', source, 'json'])
                with oa.responses.stream(model=source, input=[{'role': 'user', 'content': 'hello'}]) as stream:
                    deltas = ''.join((e.delta for e in stream if e.type == 'response.output_text.delta'))
                    final = stream.get_final_response()
                assert deltas == 'SDK hello' and final.output_text == 'SDK hello' and (final.usage.total_tokens == 8)
                checks.append(['responses', source, 'stream'])
                r = oa.chat.completions.create(model=source, messages=[{'role': 'user', 'content': 'hello'}])
                assert r.choices[0].message.content == 'SDK hello' and r.usage.total_tokens == 8
                checks.append(['chat_completions', source, 'json'])
                r = oa.chat.completions.create(model=source, messages=[{'role': 'user', 'content': 'hello'}], stream=True, stream_options={'include_usage': True})
                chunks = list(r)
                text = ''.join((c.choices[0].delta.content or '' for c in chunks if c.choices))
                assert text == 'SDK hello' and chunks[-1].usage.total_tokens == 8
                checks.append(['chat_completions', source, 'stream'])
                r = an.messages.create(model=source, max_tokens=256, messages=[{'role': 'user', 'content': 'hello'}])
                assert r.content[0].text == 'SDK hello' and r.usage.input_tokens == 3 and (r.usage.output_tokens == 5)
                checks.append(['anthropic_messages', source, 'json'])
                with an.messages.stream(model=source, max_tokens=256, messages=[{'role': 'user', 'content': 'hello'}]) as stream:
                    text = ''.join(stream.text_stream)
                    r = stream.get_final_message()
                assert text == 'SDK hello' and r.usage.input_tokens == 3 and (r.usage.output_tokens == 5)
                checks.append(['anthropic_messages', source, 'stream'])
                schema = {'type': 'object', 'properties': {'a': {'type': 'integer'}, 'b': {'type': 'integer'}}, 'required': ['a', 'b']}
                tools = [{'type': 'function', 'name': 'add', 'description': 'Add', 'parameters': schema, 'strict': False}]
                with oa.responses.stream(model=source, input=[{'role': 'user', 'content': 'compute'}], tools=tools) as stream:
                    list(stream)
                    r = stream.get_final_response()
                call = next((i for i in r.output if i.type == 'function_call'))
                assert json.loads(call.arguments) == {'a': 19, 'b': 23}
                checks.append(['responses', source, 'tool-stream'])
                chunks = list(oa.chat.completions.create(model=source, messages=[{'role': 'user', 'content': 'compute'}], tools=[{'type': 'function', 'function': {'name': 'add', 'description': 'Add', 'parameters': schema}}], stream=True, stream_options={'include_usage': True}))
                arguments = ''.join((call.function.arguments or '' for chunk in chunks if chunk.choices for call in chunk.choices[0].delta.tool_calls or []))
                assert json.loads(arguments) == {'a': 19, 'b': 23}
                checks.append(['chat_completions', source, 'tool-stream'])
                with an.messages.stream(model=source, max_tokens=256, messages=[{'role': 'user', 'content': 'compute'}], tools=[{'name': 'add', 'description': 'Add', 'input_schema': schema}]) as stream:
                    list(stream)
                    r = stream.get_final_message()
                call = next((i for i in r.content if i.type == 'tool_use'))
                assert call.input == {'a': 19, 'b': 23}
                checks.append(['anthropic_messages', source, 'tool-stream'])
                for kind in ['url', 'base64']:
                    for streaming in [False, True]:
                        for front in apis:
                            before = len(SEEN)
                            parts = image_content(front, kind)
                            if front == 'responses':
                                if streaming:
                                    with oa.responses.stream(model=source, input=[{'role': 'user', 'content': parts}]) as stream:
                                        list(stream)
                                        text = stream.get_final_response().output_text
                                else:
                                    text = oa.responses.create(model=source, input=[{'role': 'user', 'content': parts}]).output_text
                            elif front == 'chat_completions':
                                response = oa.chat.completions.create(model=source, messages=[{'role': 'user', 'content': parts}], stream=streaming)
                                text = ''.join(c.choices[0].delta.content or '' for c in response if c.choices) if streaming else response.choices[0].message.content
                            elif streaming:
                                with an.messages.stream(model=source, max_tokens=256, messages=[{'role': 'user', 'content': parts}]) as stream:
                                    text = ''.join(stream.text_stream)
                            else:
                                text = an.messages.create(model=source, max_tokens=256, messages=[{'role': 'user', 'content': parts}]).content[0].text
                            assert text == 'SDK hello'
                            assert len(SEEN) == before + 1
                            upstream_api, request = SEEN[-1]
                            history = request['input'] if source == 'responses' else request['messages']
                            forwarded = next(m['content'] for m in history if m.get('role') == 'user')
                            assert upstream_api == source and forwarded == image_content(source, kind)
                            checks.append([front, source, 'image-' + kind + ('-sse' if streaming else '-json')])
            receipt = {'openai_sdk': openai.__version__, 'anthropic_sdk': anthropic.__version__, 'strict_response_validation': True, 'passed_cases': len(checks), 'cases': checks, 'real_model_traffic': False}
            print(json.dumps(receipt))
        finally:
            p.terminate()
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                p.kill()
                p.wait()
            upstream.shutdown()
if __name__ == '__main__':
    main()
