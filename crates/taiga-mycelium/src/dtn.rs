use std::path::Path;
use redb::{Database, TableDefinition, ReadableTable};
use crate::TreeId;
use uuid::Uuid;

/// Имя таблицы, где мы будем хранить транзитные "Хвоинки" (Needles) для других узлов.
/// Ключ (Key): Целевой TreeId (кому предназначается пакет).
/// Значение (Value): Сериализованный пакет (сырые байты Хвоинки).
const TRANSIT_BUFFER: TableDefinition<[u8; 16], &[u8]> = TableDefinition::new("transit_buffer");

pub struct DtnBuffer {
    db: Database,
}

impl DtnBuffer {
    /// Инициализирует локальную базу данных на диске.
    /// `db_path`: Путь к файлу базы данных (например, "taiga_dtn.redb").
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self, String> {
        let db = Database::create(db_path).map_err(|e| format!("Ошибка создания БД: {}", e))?;
        
        // Создаем таблицу, если ее нет
        let write_txn = db.begin_write().map_err(|e| e.to_string())?;
        {
            let _ = write_txn.open_table(TRANSIT_BUFFER).map_err(|e| e.to_string())?;
        }
        write_txn.commit().map_err(|e| e.to_string())?;

        Ok(Self { db })
    }

    /// Сохраняет пакет в транзитный буфер (на диск).
    /// `target`: Кому предназначен пакет.
    /// `payload`: Зашифрованные данные или Хвоинка.
    pub fn store_transit_packet(&self, target: TreeId, payload: &[u8]) -> Result<(), String> {
        let write_txn = self.db.begin_write().map_err(|e| e.to_string())?;
        {
            let mut table = write_txn.open_table(TRANSIT_BUFFER).map_err(|e| e.to_string())?;
            // Uuid можно конвертировать в 16-байтный массив для использования как ключ в redb
            table.insert(*target.as_bytes(), payload).map_err(|e| e.to_string())?;
        }
        write_txn.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Извлекает и УДАЛЯЕТ все пакеты из буфера, предназначенные для конкретного узла.
    /// Вызывается, когда мы вдруг встретили этот узел в сети.
    pub fn take_transit_packets(&self, target: TreeId) -> Result<Vec<Vec<u8>>, String> {
        let mut packets = Vec::new();
        
        let write_txn = self.db.begin_write().map_err(|e| e.to_string())?;
        {
            let mut table = write_txn.open_table(TRANSIT_BUFFER).map_err(|e| e.to_string())?;
            let key = *target.as_bytes();
            
            // redb позволяет хранить только одно значение на уникальный ключ,
            // поэтому для хранения множества пакетов для одного узла нам придется
            // либо сериализовать Vec<Vec<u8>> в одно значение, либо использовать 
            // составной ключ (TargetID + SeqNum). 
            // В данной реализации (для простоты) мы используем TargetID как ключ.
            // Если пакет для узла есть — забираем его (удаляя из БД).
            if let Ok(Some(value)) = table.remove(&key) {
                packets.push(value.value().to_vec());
            }
        }
        write_txn.commit().map_err(|e| e.to_string())?;

        Ok(packets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_dtn_store_and_take() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        
        let dtn = DtnBuffer::new(&db_path).unwrap();
        
        let target_id = Uuid::new_v4();
        let payload1 = b"transit packet 1";
        
        dtn.store_transit_packet(target_id, payload1).unwrap();
        
        let packets = dtn.take_transit_packets(target_id).unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0], payload1);
        
        let packets_empty = dtn.take_transit_packets(target_id).unwrap();
        assert!(packets_empty.is_empty());
    }
}
