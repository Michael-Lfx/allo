import { useTranslation } from 'react-i18next';
import type { StructureView } from '../../types';
import { buildGroupPalette } from './palette';

/** 矩阵（主干 × 阶段）：读侧确定性重算的二维阵，格 = 块名清单。 */
export function MatrixView({ structure }: { structure: StructureView }) {
  const { t } = useTranslation();
  const colorFor = buildGroupPalette(structure.spines.map((spine) => spine.name));
  const columns: Array<{ key: 'entryway' | 'intermediate' | 'advanced'; label: string }> = [
    { key: 'entryway', label: t('learningGraphStructStage_entry') },
    { key: 'intermediate', label: t('learningGraphStructStage_intermediate') },
    { key: 'advanced', label: t('learningGraphStructStage_advanced') },
  ];

  return (
    <div className='overflow-auto rounded-8px border-1 border-solid border-[var(--color-border-2)]'>
      <table className='w-full border-collapse text-12px'>
        <thead>
          <tr>
            <th className='border-b border-b-solid border-b-[var(--color-border-2)] border-r border-r-solid border-r-[var(--color-border-2)] bg-[var(--color-fill-1)] px-10px py-6px text-left font-500 text-[var(--color-text-1)]'>
              {t('learningGraphStructMatrixSpine')}
            </th>
            {columns.map((column) => (
              <th
                key={column.key}
                className='border-b border-b-solid border-b-[var(--color-border-2)] bg-[var(--color-fill-1)] px-10px py-6px text-left font-500 text-[var(--color-text-1)]'
              >
                {column.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {structure.matrix.map((row) => (
            <tr key={row.spine}>
              <td className='border-b border-b-solid border-b-[var(--color-border-2)] border-r border-r-solid border-r-[var(--color-border-2)] px-10px py-6px align-top'>
                <span className='flex items-center gap-6px text-[var(--color-text-1)]'>
                  <span
                    className='inline-block h-8px w-8px rd-2px'
                    style={{ background: colorFor(row.spine) }}
                  />
                  {row.spine}
                </span>
              </td>
              {columns.map((column) => (
                <td
                  key={column.key}
                  className='border-b border-b-solid border-b-[var(--color-border-2)] px-10px py-6px align-top'
                >
                  <div className='flex flex-wrap gap-4px'>
                    {row[column.key].map((name) => (
                      <span
                        key={name}
                        className='rounded-4px border-1 border-solid border-[var(--color-border-2)] px-6px py-1px text-11px text-[var(--color-text-2)]'
                      >
                        {name}
                      </span>
                    ))}
                  </div>
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
