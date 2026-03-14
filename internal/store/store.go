package store

import (
	"encoding/binary"
	"encoding/json"
	"fmt"
	"time"

	bolt "go.etcd.io/bbolt"
)

var (
	quotaBucket   = []byte("quota")
	deviceBucket  = []byte("devices")
	configBucket  = []byte("config")
	historyBucket = []byte("history")
)

// Store wraps bbolt for SLQM persistence.
type Store struct {
	db *bolt.DB
}

// Open creates or opens the bbolt database at the given path.
func Open(path string) (*Store, error) {
	db, err := bolt.Open(path, 0600, &bolt.Options{
		Timeout: 5 * time.Second,
	})
	if err != nil {
		return nil, fmt.Errorf("open store: %w", err)
	}

	err = db.Update(func(tx *bolt.Tx) error {
		for _, b := range [][]byte{quotaBucket, deviceBucket, configBucket, historyBucket} {
			if _, err := tx.CreateBucketIfNotExists(b); err != nil {
				return err
			}
		}
		return nil
	})
	if err != nil {
		db.Close()
		return nil, fmt.Errorf("init buckets: %w", err)
	}

	return &Store{db: db}, nil
}

// Close closes the database.
func (s *Store) Close() error {
	return s.db.Close()
}

// SaveQuota persists the current monthly usage and billing month.
func (s *Store) SaveQuota(monthUsed, usedUpload, usedDownload int64, billingMonth string) error {
	return s.db.Update(func(tx *bolt.Tx) error {
		b := tx.Bucket(quotaBucket)
		b.Put([]byte("month_used"), itob(monthUsed))
		b.Put([]byte("used_upload"), itob(usedUpload))
		b.Put([]byte("used_download"), itob(usedDownload))
		b.Put([]byte("billing_month"), []byte(billingMonth))
		b.Put([]byte("last_save"), itob(time.Now().Unix()))
		return nil
	})
}

// LoadQuota reads the persisted quota state.
func (s *Store) LoadQuota() (monthUsed, usedUpload, usedDownload int64, billingMonth string, err error) {
	err = s.db.View(func(tx *bolt.Tx) error {
		b := tx.Bucket(quotaBucket)
		if v := b.Get([]byte("month_used")); v != nil {
			monthUsed = btoi(v)
		}
		if v := b.Get([]byte("used_upload")); v != nil {
			usedUpload = btoi(v)
		}
		if v := b.Get([]byte("used_download")); v != nil {
			usedDownload = btoi(v)
		}
		if v := b.Get([]byte("billing_month")); v != nil {
			billingMonth = string(v)
		}
		return nil
	})
	return
}

// SaveDeviceCycleBytes persists a device's cumulative cycle bytes.
func (s *Store) SaveDeviceCycleBytes(mac string, cycleBytes int64) error {
	return s.db.Update(func(tx *bolt.Tx) error {
		b := tx.Bucket(deviceBucket)
		return b.Put([]byte(mac+"_cycle"), itob(cycleBytes))
	})
}

// LoadDeviceCycleBytes reads a device's persisted cycle bytes.
func (s *Store) LoadDeviceCycleBytes(mac string) (int64, error) {
	var v int64
	err := s.db.View(func(tx *bolt.Tx) error {
		b := tx.Bucket(deviceBucket)
		if data := b.Get([]byte(mac + "_cycle")); data != nil {
			v = btoi(data)
		}
		return nil
	})
	return v, err
}

// SaveConfig persists the config as JSON.
func (s *Store) SaveConfig(data []byte) error {
	return s.db.Update(func(tx *bolt.Tx) error {
		b := tx.Bucket(configBucket)
		return b.Put([]byte("config_json"), data)
	})
}

// LoadConfig reads the persisted config JSON.
func (s *Store) LoadConfig() ([]byte, error) {
	var data []byte
	err := s.db.View(func(tx *bolt.Tx) error {
		b := tx.Bucket(configBucket)
		v := b.Get([]byte("config_json"))
		if v != nil {
			data = make([]byte, len(v))
			copy(data, v)
		}
		return nil
	})
	return data, err
}

// SaveHistorySnapshot saves a timestamped state snapshot for historical charting.
func (s *Store) SaveHistorySnapshot(ts time.Time, snapshot []byte) error {
	return s.db.Update(func(tx *bolt.Tx) error {
		b := tx.Bucket(historyBucket)
		return b.Put(itob(ts.Unix()), snapshot)
	})
}

// LoadHistory reads history snapshots within a time range.
func (s *Store) LoadHistory(from, to time.Time) ([]json.RawMessage, error) {
	var results []json.RawMessage
	err := s.db.View(func(tx *bolt.Tx) error {
		b := tx.Bucket(historyBucket)
		c := b.Cursor()
		start := itob(from.Unix())
		end := itob(to.Unix())
		for k, v := c.Seek(start); k != nil; k, v = c.Next() {
			if string(k) > string(end) {
				break
			}
			cp := make([]byte, len(v))
			copy(cp, v)
			results = append(results, json.RawMessage(cp))
		}
		return nil
	})
	return results, err
}

// PruneHistory removes history entries older than the given time.
func (s *Store) PruneHistory(before time.Time) error {
	return s.db.Update(func(tx *bolt.Tx) error {
		b := tx.Bucket(historyBucket)
		c := b.Cursor()
		cutoff := itob(before.Unix())
		for k, _ := c.First(); k != nil; k, _ = c.Next() {
			if string(k) >= string(cutoff) {
				break
			}
			if err := b.Delete(k); err != nil {
				return err
			}
		}
		return nil
	})
}

// ClearDevices removes all device data (used on billing cycle reset).
func (s *Store) ClearDevices() error {
	return s.db.Update(func(tx *bolt.Tx) error {
		if err := tx.DeleteBucket(deviceBucket); err != nil {
			return err
		}
		_, err := tx.CreateBucket(deviceBucket)
		return err
	})
}

func itob(v int64) []byte {
	b := make([]byte, 8)
	binary.BigEndian.PutUint64(b, uint64(v))
	return b
}

func btoi(b []byte) int64 {
	if len(b) < 8 {
		return 0
	}
	return int64(binary.BigEndian.Uint64(b))
}
