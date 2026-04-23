use std::collections::{HashMap, HashSet};
use taiga_mycelium::{Needle, TreeId};
use uuid::Uuid;

/// Разбивает большой массив данных (Шишку) на массив маленьких (Хвою).
/// 
/// `payload`: Исходные данные для отправки
/// `target_tree`: ID узла-получателя (Экзит-ноды или конечного Дерева)
/// `chunk_size`: Максимальный размер данных в одной Хвоинке (в байтах)
pub fn split_into_needles(payload: &[u8], target_tree: TreeId, chunk_size: usize) -> Vec<Needle> {
    if payload.is_empty() {
        return vec![];
    }

    let cone_id = Uuid::new_v4();
    let chunks: Vec<&[u8]> = payload.chunks(chunk_size).collect();
    let total_needles = chunks.len() as u32;

    let mut needles = Vec::with_capacity(chunks.len());
    for (i, chunk) in chunks.into_iter().enumerate() {
        needles.push(Needle {
            cone_id,
            sequence_number: i as u32,
            total_needles,
            payload: chunk.to_vec(),
            target_tree,
        });
    }

    needles
}

/// Внутренний буфер для сборки одной конкретной Шишки
struct ConeBuffer {
    total_needles: u32,
    /// Кэш фрагментов: ключ — sequence_number, значение — данные
    chunks: HashMap<u32, Vec<u8>>,
}

/// Смола (ResinAssembler) отвечает за сборку Хвоинок обратно в полноценные Шишки (данные).
/// Она накапливает фрагменты, прилетающие по разным путям или в разном порядке.
pub struct ResinAssembler {
    /// Активные буферы сборки: ключ — ID Шишки (cone_id)
    active_cones: HashMap<Uuid, ConeBuffer>,
}

impl ResinAssembler {
    pub fn new() -> Self {
        Self {
            active_cones: HashMap::new(),
        }
    }

    /// Добавляет новую Хвоинку в процесс сборки.
    /// Если после добавления Шишка собралась целиком — возвращает собранный `Vec<u8>`.
    /// Иначе возвращает `None` (ожидаем остальные части).
    pub fn receive_needle(&mut self, needle: Needle) -> Option<Vec<u8>> {
        let cone_id = needle.cone_id;
        
        let buffer = self.active_cones.entry(cone_id).or_insert_with(|| ConeBuffer {
            total_needles: needle.total_needles,
            chunks: HashMap::new(),
        });

        // Сохраняем кусок, если его еще нет (защита от дубликатов при бродкастах)
        buffer.chunks.insert(needle.sequence_number, needle.payload);

        // Проверяем, собрана ли вся Шишка
        if buffer.chunks.len() as u32 == buffer.total_needles {
            // Собираем!
            let mut full_payload = Vec::new();
            for i in 0..buffer.total_needles {
                // Извлекаем куски по порядку
                if let Some(chunk) = buffer.chunks.get(&i) {
                    full_payload.extend_from_slice(chunk);
                } else {
                    // Ситуация "невозможно", так как мы проверили длину, но для безопасности
                    return None; 
                }
            }

            // Очищаем буфер, так как Шишка собрана
            self.active_cones.remove(&cone_id);

            return Some(full_payload);
        }

        None
    }

    /// Очистка "зависших" пакетов. 
    /// В реальной сети часть Хвои может потеряться, и буфер будет висеть вечно.
    /// (В будущем тут нужно добавить проверку по тайм-ауту)
    pub fn clear_abandoned(&mut self) {
        // Заглушка для будущего GC (Garbage Collector)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_split_and_assemble() {
        let target = Uuid::from_str("00000000-0000-0000-0000-000000000001").unwrap();
        let message = b"Hello, this is a very long message that should be split into multiple needles by Taiga Resin!";
        
        // Режем на куски по 10 байт
        let mut needles = split_into_needles(message, target, 10);
        assert!(needles.len() > 1);

        let mut assembler = ResinAssembler::new();
        let mut result = None;

        // Эмулируем случайный порядок прихода пакетов по сети (перемешиваем)
        needles.reverse();

        for needle in needles {
            result = assembler.receive_needle(needle);
        }

        // В конце результат не должен быть None
        assert!(result.is_some());
        
        let assembled_message = result.unwrap();
        assert_eq!(message.to_vec(), assembled_message);
        
        // Проверяем, что буфер очистился
        assert!(assembler.active_cones.is_empty());
    }
}
