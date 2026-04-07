import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";
import { cluster, logGroup, service } from "./backend";
import { stack } from "./config";
import { apiGateway } from "./network";

// ---------------------------------------------------------------------------
// CloudWatch Observability
// ---------------------------------------------------------------------------
const alarmTopic = new aws.sns.Topic("sentinel-alarm-topic", {
  name: `sentinel-${stack}-alarms`,
});

new aws.sns.TopicSubscription("sentinel-alarm-email", {
  topic: alarmTopic.arn,
  protocol: "email",
  endpoint: "matt@zireael.dev",
});

const errNs = `Sentinel/${stack}`;
const errorFilter = new aws.cloudwatch.LogMetricFilter("app-error-filter", {
  name: `sentinel-${stack}-app-errors`,
  logGroupName: logGroup.name,
  pattern: '{ $.level = "ERROR" }',
  metricTransformation: {
    namespace: errNs,
    name: "AppErrors",
    value: "1",
    defaultValue: "0",
    unit: "Count",
  },
});

new aws.cloudwatch.MetricAlarm(
  "app-error-alarm",
  {
    name: `sentinel-${stack}-app-errors`,
    alarmDescription: "Backend logged an ERROR — check CloudWatch Logs",
    namespace: errNs,
    metricName: "AppErrors",
    statistic: "Sum",
    period: 60,
    evaluationPeriods: 1,
    threshold: 1,
    comparisonOperator: "GreaterThanOrEqualToThreshold",
    treatMissingData: "notBreaching",
    alarmActions: [alarmTopic.arn],
    okActions: [alarmTopic.arn],
  },
  { dependsOn: [errorFilter] },
);

// Low gas balance warning — fires when the publisher logs LOW GAS BALANCE
const gasFilter = new aws.cloudwatch.LogMetricFilter("low-gas-filter", {
  name: `sentinel-${stack}-low-gas`,
  logGroupName: logGroup.name,
  pattern: '"LOW GAS BALANCE"',
  metricTransformation: {
    namespace: errNs,
    name: "LowGasWarnings",
    value: "1",
    defaultValue: "0",
    unit: "Count",
  },
});

new aws.cloudwatch.MetricAlarm(
  "low-gas-alarm",
  {
    name: `sentinel-${stack}-low-gas`,
    alarmDescription:
      "Publisher wallet gas balance is low — fund with testnet SUI",
    namespace: errNs,
    metricName: "LowGasWarnings",
    statistic: "Sum",
    period: 3600,
    evaluationPeriods: 1,
    threshold: 1,
    comparisonOperator: "GreaterThanOrEqualToThreshold",
    treatMissingData: "notBreaching",
    alarmActions: [alarmTopic.arn],
  },
  { dependsOn: [gasFilter] },
);

new aws.cloudwatch.MetricAlarm("task-down-alarm", {
  name: `sentinel-${stack}-task-down`,
  alarmDescription:
    "ECS running task count at 0 for 10+ minutes — may need attention",
  namespace: "ECS/ContainerInsights",
  metricName: "RunningTaskCount",
  statistic: "Minimum",
  period: 60,
  evaluationPeriods: 10,
  threshold: 1,
  comparisonOperator: "LessThanThreshold",
  treatMissingData: "breaching",
  dimensions: {
    ClusterName: cluster.name,
    ServiceName: service.name,
  },
  alarmActions: [alarmTopic.arn],
});

new aws.cloudwatch.MetricAlarm("api-5xx-alarm", {
  name: `sentinel-${stack}-api-5xx`,
  alarmDescription: "API Gateway returning 5xx errors",
  namespace: "AWS/ApiGateway",
  metricName: "5xx",
  statistic: "Sum",
  period: 300,
  evaluationPeriods: 2,
  threshold: 10,
  comparisonOperator: "GreaterThanOrEqualToThreshold",
  treatMissingData: "notBreaching",
  dimensions: { ApiId: apiGateway.id },
  alarmActions: [alarmTopic.arn],
  okActions: [alarmTopic.arn],
});

new aws.cloudwatch.MetricAlarm("api-4xx-alarm", {
  name: `sentinel-${stack}-api-4xx`,
  alarmDescription: "Elevated 4xx client errors",
  namespace: "AWS/ApiGateway",
  metricName: "4xx",
  statistic: "Sum",
  period: 300,
  evaluationPeriods: 3,
  threshold: 50,
  comparisonOperator: "GreaterThanOrEqualToThreshold",
  treatMissingData: "notBreaching",
  dimensions: { ApiId: apiGateway.id },
  alarmActions: [alarmTopic.arn],
  okActions: [alarmTopic.arn],
});

new aws.cloudwatch.MetricAlarm("api-latency-alarm", {
  name: `sentinel-${stack}-api-latency`,
  alarmDescription: "API Gateway p99 latency elevated",
  namespace: "AWS/ApiGateway",
  metricName: "Latency",
  extendedStatistic: "p99",
  period: 300,
  evaluationPeriods: 3,
  threshold: 5000,
  comparisonOperator: "GreaterThanOrEqualToThreshold",
  treatMissingData: "notBreaching",
  dimensions: { ApiId: apiGateway.id },
  alarmActions: [alarmTopic.arn],
  okActions: [alarmTopic.arn],
});

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------
const region = aws.getRegion();

const dashboardBody = pulumi
  .all([
    apiGateway.id,
    cluster.name,
    service.name,
    pulumi.output(region).apply((r) => r.region),
  ])
  .apply(([apiId, ecsCluster, ecsSvc, reg]) => {
    return JSON.stringify({
      widgets: [
        {
          type: "metric",
          x: 0,
          y: 0,
          width: 12,
          height: 6,
          properties: {
            title: "API 5xx / 4xx Errors",
            region: reg,
            metrics: [
              [
                "AWS/ApiGateway",
                "5xx",
                "ApiId",
                apiId,
                { stat: "Sum", color: "#d62728", label: "5xx" },
              ],
              [
                "AWS/ApiGateway",
                "4xx",
                "ApiId",
                apiId,
                { stat: "Sum", color: "#ff7f0e", label: "4xx" },
              ],
            ],
            period: 300,
            view: "timeSeries",
            stacked: false,
          },
        },
        {
          type: "metric",
          x: 12,
          y: 0,
          width: 12,
          height: 6,
          properties: {
            title: "Request Count",
            region: reg,
            metrics: [
              [
                "AWS/ApiGateway",
                "Count",
                "ApiId",
                apiId,
                { stat: "Sum", label: "Requests" },
              ],
            ],
            period: 300,
            view: "timeSeries",
          },
        },
        {
          type: "metric",
          x: 0,
          y: 6,
          width: 12,
          height: 6,
          properties: {
            title: "API Latency (p99 / avg)",
            region: reg,
            metrics: [
              [
                "AWS/ApiGateway",
                "Latency",
                "ApiId",
                apiId,
                { stat: "p99", label: "p99" },
              ],
              [
                "AWS/ApiGateway",
                "Latency",
                "ApiId",
                apiId,
                { stat: "Average", label: "avg" },
              ],
            ],
            period: 300,
            view: "timeSeries",
          },
        },
        {
          type: "metric",
          x: 12,
          y: 6,
          width: 12,
          height: 6,
          properties: {
            title: "Integration Latency (p99 / avg)",
            region: reg,
            metrics: [
              [
                "AWS/ApiGateway",
                "IntegrationLatency",
                "ApiId",
                apiId,
                { stat: "p99", label: "p99" },
              ],
              [
                "AWS/ApiGateway",
                "IntegrationLatency",
                "ApiId",
                apiId,
                { stat: "Average", label: "avg" },
              ],
            ],
            period: 300,
            view: "timeSeries",
          },
        },
        {
          type: "metric",
          x: 0,
          y: 12,
          width: 12,
          height: 6,
          properties: {
            title: "ECS CPU Utilization",
            region: reg,
            metrics: [
              [
                "AWS/ECS",
                "CPUUtilization",
                "ClusterName",
                ecsCluster,
                "ServiceName",
                ecsSvc,
                { stat: "Average", label: "CPU %" },
              ],
            ],
            period: 300,
            view: "timeSeries",
          },
        },
        {
          type: "metric",
          x: 12,
          y: 12,
          width: 12,
          height: 6,
          properties: {
            title: "ECS Memory Utilization",
            region: reg,
            metrics: [
              [
                "AWS/ECS",
                "MemoryUtilization",
                "ClusterName",
                ecsCluster,
                "ServiceName",
                ecsSvc,
                { stat: "Average", label: "Memory %" },
              ],
            ],
            period: 300,
            view: "timeSeries",
          },
        },
        {
          type: "metric",
          x: 0,
          y: 18,
          width: 12,
          height: 6,
          properties: {
            title: "Running Tasks (Spot interruptions show as dips)",
            region: reg,
            metrics: [
              [
                "ECS/ContainerInsights",
                "RunningTaskCount",
                "ClusterName",
                ecsCluster,
                "ServiceName",
                ecsSvc,
                { stat: "Minimum", color: "#2ca02c", label: "Running" },
              ],
            ],
            period: 60,
            view: "timeSeries",
            yAxis: { left: { min: 0, max: 2 } },
          },
        },
        {
          type: "metric",
          x: 12,
          y: 18,
          width: 12,
          height: 6,
          properties: {
            title: "Application Errors (ERROR log level)",
            region: reg,
            metrics: [
              [
                errNs,
                "AppErrors",
                { stat: "Sum", color: "#d62728", label: "Errors" },
              ],
            ],
            period: 60,
            view: "timeSeries",
          },
        },
      ],
    });
  });

new aws.cloudwatch.Dashboard("sentinel-dashboard", {
  dashboardName: `sentinel-${stack}`,
  dashboardBody,
});
