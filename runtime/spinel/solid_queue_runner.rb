# frozen_string_literal: true
# Not the in-process list: on solid_queue Rails writes each job to solid_queue's tables and a worker claims it there.
module ActiveJob
  def self.enqueue_payload_on(queue, json)
    SolidQueueRunner.enqueue(queue, JSON.parse(json))
    nil
  end

  def self.drain
    SolidQueueRunner.drain
  end

  def self.pending_count
    SolidQueue::ReadyExecution.count
  end

  class Base
    # Rails' `ActiveJob::Base.execute(job_data)`, which a claimed execution calls: the job runs, and what it raises reaches solid_queue.
    def self.execute(job_data)
      name = job_data["job_class"].to_s
      begin
        raise NameError, "uninitialized constant #{name}" unless JobRegistry.perform(name, job_data["arguments"])
      rescue Exception => e
        gone = e.is_a?(ActiveJob::DeserializationError)
        handler = JobRegistry.handler_for(name, e)
        return nil if handler < 0 || (handler == 0 && gone && JobRegistry.discards_deserialization_error(name))
        raise e
      end
      nil
    end
  end
end

# Not solid_queue's supervisor and its forked worker: one worker, registered in this process, runs when the app's drain does.
module SolidQueueRunner
  WORKER = []

  def self.enqueue(queue, data)
    SolidQueue::Job.create!(
      queue_name: queue == "" ? "default" : queue,
      active_job_id: data["job_id"].to_s,
      priority: data["priority"].to_i,
      scheduled_at: Time.now,
      class_name: data["job_class"].to_s,
      arguments: data,
      concurrency_key: nil
    )
  end

  def self.process
    if WORKER.empty?
      WORKER << SolidQueue::Process.register(
        kind: "Worker",
        # Not `::Process`: spinel reads the rooted name as solid_queue's own `SolidQueue::Process` model (spinel#10794).
        name: "worker-#{Process.pid}",
        pid: Process.pid,
        hostname: "localhost",
        metadata: { "queues" => "*" }
      )
    end
    WORKER[0]
  end

  def self.drain
    ran = 0
    SolidQueue::ScheduledExecution.dispatch_next_batch(100)
    loop do
      claimed = SolidQueue::ReadyExecution.claim("*", 1, process.id)
      break if claimed.empty?
      claimed.each do |execution|
        begin
          execution.perform
        rescue StandardError => e
          warn "[job] a queued job raised: " + e.message
        end
        ran += 1
      end
    end
    ran
  end
end
