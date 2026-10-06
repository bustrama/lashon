export interface AgentActivity {
    agent: string;
    state: 'working' | 'tool' | 'done' | 'stopped' | 'error';
    tool: string | null;
    count: number;
}
