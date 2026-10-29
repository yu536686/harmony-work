import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, it } from 'vitest';
import { TaskProgress, UploadProgress } from './Progress';
afterEach(cleanup);
it('does not invent a percentage for unknown task progress', () => {
  render(<TaskProgress task={{ id: '1', status: 'running', stage: '正在查询来源' }} />);
  expect(screen.getByRole('progressbar')).not.toHaveAttribute('value');
  expect(screen.getByRole('status')).toHaveTextContent('正在查询来源');
});
it('distinguishes transmitted bytes from confirmed saved results', () => {
  render(<UploadProgress task={{ id: '1', source: 'watch', fileName: 'a', folder: 'b', size: 100, uploaded: 100, status: 'saving' }} />);
  expect(screen.getByRole('status')).toHaveTextContent('正在保存与入库');
  expect(screen.queryByText('已完成保存与入库')).not.toBeInTheDocument();
});
