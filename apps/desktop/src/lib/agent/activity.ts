export interface AgentActivity {
    id: number;
    agent: string;
    state: 'working' | 'tool' | 'done' | 'stopped' | 'error';
    tool: string | null;
}
