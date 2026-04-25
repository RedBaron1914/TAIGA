use std::path::Path;
use redb::{Database, MultimapTableDefinition, ReadableMultimapTable};
use crate::TreeId;
use std::time::{SystemTime, UNIX_EPOCH};

/// Имя таблицы, где мы будем хранить транзитные "Хвоинки" (Needles) для других узлов.
/// Ключ (Key): Целевой TreeId (кому предназначается пакет).
/// Значение (Value): Сериализованный пакет с меткой времени (8 байт timestamp + сырые байты Хвоинки).
const TRANSIT_BUFFER: MultimapTableDefinition<[u8; 16], &[u8]> = MultimapTableDefinition::new("transit_buffer");

pub struct DtnBuffer {
    db: Database,
    pub path: std::path::PathBuf,
}

impl DtnBuffer {
    /// Инициализирует локальную базу данных на диске.
    /// `db_path`: Путь к файлу базы данных (например, "taiga_dtn.redb").
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self, String> {
        let path = db_path.as_ref().to_path_buf();
        let db = Database::create(db_path).map_err(|e| format!("Ошибка создания БД: {}", e))?;
        
        // Создаем таблицу, если ее нет
        let write_txn = db.begin_write().map_err(|e| e.to_string())?;
        {
            let _ = write_txn.open_multimap_table(TRANSIT_BUFFER).map_err(|e| e.to_string())?;
        }
        write_txn.commit().map_err(|e| e.to_string())?;

        Ok(Self { db, path })
    }

    /// Сохраняет пакет в транзитный буфер (на диск) с текущей меткой времени.
    pub fn store_transit_packet(&self, target: TreeId, payload: &[u8]) -> Result<(), String> {
        let write_txn = self.db.begin_write().map_err(|e| e.to_string())?;
        {
            let mut table = write_txn.open_multimap_table(TRANSIT_BUFFER).map_err(|e| e.to_string())?;
            
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            let mut stored_payload = Vec::with_capacity(8 + payload.len());
            stored_payload.extend_from_slice(&now.to_be_bytes());
            stored_payload.extend_from_slice(payload);
            
            table.insert(*target.as_bytes(), stored_payload.as_slice()).map_err(|e| e.to_string())?;
        }
        write_txn.commit().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Извлекает и УДАЛЯЕТ все актуальные пакеты из буфера. Устаревшие отбрасываются.
    pub fn take_transit_packets(&self, target: TreeId) -> Result<Vec<Vec<u8>>, String> {
        let mut packets = Vec::new();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let ttl_secs = 86400 * 7; // 7 дней хранения
        
        let write_txn = self.db.begin_write().map_err(|e| e.to_string())?;
        {
            let mut table = write_txn.open_multimap_table(TRANSIT_BUFFER).map_err(|e| e.to_string())?;
            let key = *target.as_bytes();
            
            // Извлекаем и удаляем все значения по ключу
            if let Ok(mut iter) = table.remove_all(&key) {
                while let Some(Ok(value)) = iter.next() {
                    let data = value.value();
                    if data.len() >= 8 {
                        let timestamp = u64::from_be_bytes(data[0..8].try_into().unwrap());
                        if now.saturating_sub(timestamp) <= ttl_secs {
                            packets.push(data[8..].to_vec());
                        }
                    }
                }
            }
        }
        write_txn.commit().map_err(|e| e.to_string())?;

        Ok(packets)
    }
    
    /// Очищает все устаревшие пакеты из базы данных для экономии места.
    pub fn cleanup_expired(&self) -> Result<usize, String> {
        let mut removed_count = 0;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let ttl_secs = 86400 * 7; // 7 дней
        
        let write_txn = self.db.begin_write().map_err(|e| e.to_string())?;
        {
            let mut table = write_txn.open_multimap_table(TRANSIT_BUFFER).map_err(|e| e.to_string())?;
            // Мы не можем легко удалять конкретные значения из MultimapTable во время итерации по ключам, 
            // поэтому для сборки мусора мы перебираем все ключи и пересохраняем только валидные данные.
            // В реальном production-коде здесь нужна более сложная логика миграции, 
            // но для беты мы просто очищаем полностью устаревшие ключи.
            let mut expired_keys = Vec::new();
            if let Ok(iter) = table.iter() {
                for entry in iter {
                    if let Ok((key, values)) = entry {
                        let mut all_expired = true;
                        for value in values {
                            if let Ok(val) = value {
                                let data = val.value();
                                if data.len() >= 8 {
                                    let timestamp = u64::from_be_bytes(data[0..8].try_into().unwrap());
                                    if now.saturating_sub(timestamp) <= ttl_secs {
                                        all_expired = false;
                                        break;
                                    }
                                }
                            }
                        }
                        if all_expired {
                            expired_keys.push(key.value());
                        }
                    }
                }
            }
            
            for key in expired_keys {
                let _ = table.remove_all(&key);
                removed_count += 1;
            }
        }
        write_txn.commit().map_err(|e| e.to_string())?;
        
        Ok(removed_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn test_dtn_store_and_take() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.redb");
        
        let dtn = DtnBuffer::new(&db_path).unwrap();
        
        let target_id = Uuid::new_v4();
        let payload1 = b"transit packet 1";
        let payload2 = b"transit packet 2";
        
        dtn.store_transit_packet(target_id, payload1).unwrap();
        dtn.store_transit_packet(target_id, payload2).unwrap();
        
        let packets = dtn.take_transit_packets(target_id).unwrap();
        assert_eq!(packets.len(), 2);
        // Multimap entries order might not be strictly insertion order if not sorted, but redb preserves order in some versions.
        // We just check if both exist.
        assert!(packets.contains(&payload1.to_vec()));
        assert!(packets.contains(&payload2.to_vec()));
        
        let packets_empty = dtn.take_transit_packets(target_id).unwrap();
        assert!(packets_empty.is_empty());
    }
}
